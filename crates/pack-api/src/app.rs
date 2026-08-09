use anyhow::{Result, bail};
use pack_core::client::context::{
    get_client_module_options_context, get_client_resolve_options_context,
    get_client_runtime_entries,
};
use pack_core::config::{Platform, ServerEntry};
use pack_core::server_reference::server_reference_module::ServerReferenceModule;
use pack_core::server_reference::server_reference_transition::ServerReferenceTransition;
use rustc_hash::{FxHashMap, FxHashSet};

use pack_core::server::contexts::{
    get_server_module_options_context, get_server_resolve_options_context,
};
use pack_core::util::convert_to_project_relative;
use tracing::Instrument;
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{Completion, JoinIterExt, ResolvedVc, TryJoinIterExt, ValueToString, Vc};
use turbopack::{
    ModuleAssetContext, module_options::ModuleOptionsContext, transition::TransitionOptions,
};
use turbopack_core::chunk::ChunkingContextExt;
use turbopack_core::output::OutputAssetsWithReferenced;
use turbopack_core::resolve::origin::ResolveOrigin;
use turbopack_core::{
    chunk::{
        ChunkableModule, ChunkingContext, EvaluatableAsset, EvaluatableAssets,
        availability_info::AvailabilityInfo,
    },
    context::AssetContext,
    ident::{AssetIdent, Layer},
    module::{Module, Modules},
    module_graph::{
        GraphEntries, GraphTraversalAction, ModuleGraph,
        chunk_group_info::{ChunkGroup, ChunkGroupEntry, EntryHeuristics},
    },
    output::OutputAssets,
    reference_type::{EntryReferenceSubType, ReferenceType},
    resolve::{origin::PlainResolveOrigin, parse::Request},
};

use crate::{
    endpoint::{Endpoint, EndpointOutput, EndpointOutputPaths},
    paths::initial_paths_in_root,
    project::Project,
};
use turbopack_resolve::resolve_options_context::ResolveOptionsContext;

#[turbo_tasks::value(transparent)]
pub struct AppEntrypoints(pub Vec<AppEntrypoint>);

#[turbo_tasks::value]
pub struct AppProject {
    pub project: ResolvedVc<Project>,
    pub apps: ResolvedVc<AppEntrypoints>,
}

#[turbo_tasks::value(transparent)]
pub struct OptionAppProject(Option<ResolvedVc<AppProject>>);

#[turbo_tasks::value_impl]
impl AppProject {
    #[turbo_tasks::function]
    pub fn new(project: ResolvedVc<Project>, apps: ResolvedVc<AppEntrypoints>) -> Vc<Self> {
        Self { project, apps }.cell()
    }

    #[turbo_tasks::function]
    pub fn apps(&self) -> Vc<AppEntrypoints> {
        *self.apps
    }

    #[turbo_tasks::function]
    pub async fn get_app_endpoint(self: Vc<Self>) -> Result<Vc<AppEndpoint>> {
        let this = self.await?;

        let project = this.project;

        let entrypoints = this
            .apps
            .await?
            .iter()
            .map(|a| async move {
                AppEntrypoint {
                    project,
                    name: a.name.clone(),
                    import: a.import.clone(),
                }
                .resolved_cell()
            })
            .join()
            .await;

        Ok(AppEndpoint {
            project,
            entrypoints,
        }
        .cell())
    }
}

#[turbo_tasks::value]
pub struct AppEntrypoint {
    pub project: ResolvedVc<Project>,
    pub name: RcStr,
    pub import: RcStr,
}

#[turbo_tasks::value_impl]
impl AppEntrypoint {
    #[turbo_tasks::function]
    fn project(&self) -> Vc<Project> {
        *self.project
    }

    #[turbo_tasks::function]
    pub async fn app_entry_modules(
        self: Vc<Self>,
        asset_context: Vc<Box<dyn AssetContext>>,
    ) -> Result<Vc<Modules>> {
        let this = self.await?;

        // Handle import path: convert absolute path to relative, keep relative path as-is
        let relative_import =
            convert_to_project_relative(&this.import, &self.project().project_path().await?.path)?;

        let entry_request = Request::relative(
            relative_import.into(),
            Default::default(),
            Default::default(),
            false,
        );

        let origin = PlainResolveOrigin::new(
            asset_context,
            self.project().project_path().await?.join("_")?,
        )
        .await?;
        let resolve_options = origin.resolve_options();
        let asset_context = origin.asset_context();
        let origin_path = origin.origin_path();

        let ty = ReferenceType::Entry(EntryReferenceSubType::Undefined);

        Ok(Vc::cell(
            asset_context
                .resolve_asset(origin_path, entry_request, resolve_options, ty)
                .await?
                .primary_modules()
                .await?,
        ))
    }

    #[turbo_tasks::function]
    pub async fn entry_evaluatable_assets(
        self: Vc<Self>,
        asset_context: Vc<Box<dyn AssetContext>>,
        runtime_entries: Vc<EvaluatableAssets>,
    ) -> Result<Vc<EvaluatableAssets>> {
        let runtime_entries = runtime_entries.await?;
        let modules = self.app_entry_modules(asset_context).await?;

        let mut all_runtime_entries = Vec::with_capacity(modules.len() + runtime_entries.len());

        all_runtime_entries.extend(runtime_entries.iter().map(|e| **e));

        for &module in &modules {
            if let Some(entry) = ResolvedVc::try_downcast::<Box<dyn EvaluatableAsset>>(module) {
                all_runtime_entries.push(*entry);
            } else {
                bail!(
                    "runtime reference resolved to an asset ({}) that cannot be evaluated",
                    module.ident().to_string().await?
                );
            }
        }

        Ok(EvaluatableAssets::many(all_runtime_entries))
    }

    #[turbo_tasks::function]
    pub async fn module_graph_for_entry(
        self: Vc<Self>,
        asset_context: Vc<Box<dyn AssetContext>>,
        runtime_entries: Vc<EvaluatableAssets>,
    ) -> Result<Vc<ModuleGraph>> {
        let project = self.project();

        let evaluatable_assets = self.entry_evaluatable_assets(asset_context, runtime_entries);

        Ok(project.module_graph_for_modules(evaluatable_assets))
    }

    #[turbo_tasks::function]
    async fn client_chunk_group(
        self: Vc<Self>,
        asset_context: Vc<Box<dyn AssetContext>>,
        runtime_entries: Vc<EvaluatableAssets>,
    ) -> Result<Vc<OutputAssetsWithReferenced>> {
        async move {
            let this = self.await?;

            let project = self.project();

            let module_graph = self.module_graph_for_entry(asset_context, runtime_entries);

            let query = format!("?name={}", this.name);

            let app_chunk_group = project
                .client_chunking_context()
                .evaluated_chunk_group_assets(
                    AssetIdent::from_path(
                        project.project_path().await?.join(this.import.as_str())?,
                    )
                    .with_query(query.into())
                    .into_vc(),
                    ChunkGroup::Entry(
                        self.entry_evaluatable_assets(asset_context, runtime_entries)
                            .await?
                            .iter()
                            .map(|m| ResolvedVc::upcast(*m))
                            .collect(),
                    ),
                    module_graph,
                    OutputAssets::empty(),
                    AvailabilityInfo::root(),
                );

            Ok(app_chunk_group)
        }
        .instrument(tracing::trace_span!("app chunk rendering"))
        .await
    }

    #[turbo_tasks::function]
    async fn server_chunk_group(
        self: Vc<Self>,
        asset_context: Vc<Box<dyn AssetContext>>,
        runtime_entries: Vc<EvaluatableAssets>,
    ) -> Result<Vc<OutputAssetsWithReferenced>> {
        async move {
            let this = self.await?;

            let project = self.project();

            let module_graph = self.module_graph_for_entry(asset_context, runtime_entries);

            let name = if this.name.ends_with(".js") {
                this.name.as_str()
            } else {
                &format!("{}.js", this.name)
            };

            let app_chunk_group = project
                .server_chunking_context()
                .entry_chunk_group(
                    project.dist_root().owned().await?.join(name)?,
                    ChunkGroup::Entry(
                        self.entry_evaluatable_assets(asset_context, runtime_entries)
                            .await?
                            .iter()
                            .map(|m| ResolvedVc::upcast(*m))
                            .collect(),
                    ),
                    module_graph,
                    OutputAssets::empty(),
                    OutputAssets::empty(),
                    AvailabilityInfo::root(),
                )
                .await?;

            Ok(OutputAssetsWithReferenced {
                assets: ResolvedVc::cell(vec![app_chunk_group.asset]),
                referenced_assets: ResolvedVc::cell(vec![]),
                references: ResolvedVc::cell(vec![]),
            }
            .cell())
        }
        .instrument(tracing::trace_span!("app chunk rendering"))
        .await
    }

    #[turbo_tasks::function]
    pub async fn output_assets_for_entry(
        self: Vc<Self>,
        asset_context: Vc<Box<dyn AssetContext>>,
        runtime_entries: Vc<EvaluatableAssets>,
    ) -> Result<Vc<OutputAssets>> {
        let chunk_group_assets = match &*self.project().platform().await? {
            Platform::Node => {
                *self
                    .server_chunk_group(asset_context, runtime_entries)
                    .await?
                    .assets
            }
            Platform::Web => {
                *self
                    .client_chunk_group(asset_context, runtime_entries)
                    .await?
                    .assets
            }
        };
        Ok(chunk_group_assets)
    }
}

#[turbo_tasks::value]
pub struct AppEndpoint {
    project: ResolvedVc<Project>,
    pub entrypoints: Vec<ResolvedVc<AppEntrypoint>>,
}

#[turbo_tasks::value_impl]
impl AppEndpoint {
    #[turbo_tasks::function]
    pub fn project(&self) -> Vc<Project> {
        *self.project
    }

    #[turbo_tasks::function]
    pub async fn app_runtime_entries(self: Vc<Self>) -> Result<Vc<EvaluatableAssets>> {
        let project = self.project();
        match &*project.platform().await? {
            Platform::Node => Ok(EvaluatableAssets::empty()),
            Platform::Web => {
                let watch = project.await?.watch.enable;
                Ok(get_client_runtime_entries(
                    project.project_path().owned().await?,
                    project.mode(),
                    project.config(),
                    project.execution_context(),
                    project.pack_path().owned().await?,
                    Vc::cell(watch),
                    project.client_hmr_enabled(),
                )
                .resolve_entries(Vc::upcast(self.app_module_context())))
            }
        }
    }

    #[turbo_tasks::function]
    pub async fn app_module_context(self: Vc<Self>) -> Result<Vc<ModuleAssetContext>> {
        let project = self.project();
        let platform = &*project.platform().await?;

        let layer = match platform {
            Platform::Node => {
                Layer::new_with_user_friendly_name(rcstr!("server"), rcstr!("Nodejs"))
            }
            Platform::Web => {
                Layer::new_with_user_friendly_name(rcstr!("client"), rcstr!("Browser"))
            }
        };

        // Build transition options, registering "server-reference" when configured
        let mut named_transitions: FxHashMap<
            RcStr,
            ResolvedVc<Box<dyn turbopack::transition::Transition>>,
        > = FxHashMap::default();
        let server_config = project.config().server().await?;
        if server_config.function.is_some() {
            let server_module_options_context = get_server_module_options_context(
                project.project_path().owned().await?,
                project.execution_context(),
                project.server_compile_time_info().environment(),
                project.mode(),
                project.config(),
            )
            .to_resolved()
            .await?;
            let server_resolve_options_context = get_server_resolve_options_context(
                project.project_path().owned().await?,
                project.mode(),
                project.config(),
                project.execution_context(),
                project.pack_path().owned().await?,
            )
            .to_resolved()
            .await?;
            let transition = ServerReferenceTransition::new(
                *project.server_compile_time_info().to_resolved().await?,
                *server_module_options_context,
                *server_resolve_options_context,
            )
            .to_resolved()
            .await?;
            named_transitions.insert(rcstr!("server-reference"), ResolvedVc::upcast(transition));
        }

        let transition_options = TransitionOptions {
            named_transitions,
            ..Default::default()
        }
        .cell();

        Ok(ModuleAssetContext::new(
            transition_options,
            project.compile_time_info_for_platform(),
            self.app_module_options_context(),
            self.app_resolve_options_context(),
            layer,
        ))
    }

    #[turbo_tasks::function]
    async fn app_module_options_context(self: Vc<Self>) -> Result<Vc<ModuleOptionsContext>> {
        let project = self.project();
        match &*project.platform().await? {
            Platform::Node => Ok(get_server_module_options_context(
                project.project_path().owned().await?,
                project.execution_context(),
                project.server_compile_time_info().environment(),
                project.mode(),
                project.config(),
            )),
            Platform::Web => Ok(get_client_module_options_context(
                project.project_path().owned().await?,
                project.execution_context(),
                project.client_compile_time_info().environment(),
                project.mode(),
                project.config(),
                Vc::cell(project.await?.watch.enable),
                project.pack_path().owned().await?,
            )),
        }
    }

    #[turbo_tasks::function]
    async fn app_resolve_options_context(self: Vc<Self>) -> Result<Vc<ResolveOptionsContext>> {
        let project = self.project();
        match &*project.platform().await? {
            Platform::Node => Ok(get_server_resolve_options_context(
                project.project_path().owned().await?,
                project.mode(),
                project.config(),
                project.execution_context(),
                project.pack_path().owned().await?,
            )),
            Platform::Web => Ok(get_client_resolve_options_context(
                project.project_path().owned().await?,
                project.mode(),
                project.config(),
                project.execution_context(),
                project.pack_path().owned().await?,
            )),
        }
    }
}

#[turbo_tasks::value_impl]
impl Endpoint for AppEndpoint {
    #[turbo_tasks::function]
    async fn entries(self: Vc<Self>) -> Result<Vc<GraphEntries>> {
        let this = self.await?;
        let asset_context = self.app_module_context();
        let runtime_entries = self.app_runtime_entries();

        let entries = this
            .entrypoints
            .iter()
            .map(|e| async {
                let entry_modules = e
                    .entry_evaluatable_assets(Vc::upcast(asset_context), runtime_entries)
                    .await?
                    .iter()
                    .copied()
                    .map(ResolvedVc::upcast)
                    .collect();

                Ok(ChunkGroupEntry::Entry {
                    modules: entry_modules,
                    heuristics: EntryHeuristics::default(),
                })
            })
            .try_join()
            .await?;

        Ok(GraphEntries::from_chunk_groups(entries).cell())
    }

    #[turbo_tasks::function]
    async fn output(self: Vc<Self>) -> Result<Vc<EndpointOutput>> {
        async move {
            let asset_context = self.app_module_context();

            let runtime_entries = self.app_runtime_entries();

            let this = self.await?;
            let output_assets = {
                let mut vcs = this
                    .entrypoints
                    .iter()
                    .map(|e| e.output_assets_for_entry(Vc::upcast(asset_context), runtime_entries))
                    .collect::<Vec<_>>();
                vcs.push(this.project.copy_output_assets());
                OutputAssets::concat(vcs)
            };

            // Build server functions as Node.js if configured
            let server_config = this.project.config().server().await?;
            let server_output = if server_config.function.is_some()
                || server_config
                    .entry
                    .as_ref()
                    .is_some_and(|entry| entry.has_entries())
            {
                Some(
                    self.server_reference_output_assets(Vc::upcast(asset_context), runtime_entries),
                )
            } else {
                None
            };

            let dist_root_vc = this.project.dist_root();
            let dist_root = dist_root_vc.await?;
            let client_paths = initial_paths_in_root(output_assets, dist_root_vc)
                .await?
                .iter()
                .cloned()
                .collect();

            let written_endpoint = EndpointOutputPaths::NodeJs {
                server_entry_path: dist_root.path.clone(),
                server_paths: vec![],
                client_paths,
            };

            let mut output_assets = output_assets;

            if let Some(server_output) = server_output {
                output_assets = output_assets.concatenate(server_output);
            }

            Ok(EndpointOutput {
                output_assets: output_assets.to_resolved().await?,
                output_paths: written_endpoint.resolved_cell(),
                project: this.project,
            }
            .cell())
        }
        .instrument(tracing::trace_span!("app_output"))
        .await
    }

    #[turbo_tasks::function]
    async fn server_changed(self: Vc<Self>) -> Result<Vc<Completion>> {
        let EndpointOutput {
            output_assets,
            project,
            ..
        } = *self.output().await?;
        Ok(project.server_changed(*output_assets))
    }

    #[turbo_tasks::function]
    async fn client_changed(self: Vc<Self>) -> Result<Vc<Completion>> {
        let EndpointOutput {
            output_assets,
            project,
            ..
        } = *self.output().await?;
        Ok(project.client_changed(*output_assets))
    }
}

/// Server function build support
#[turbo_tasks::value_impl]
impl AppEndpoint {
    /// Discovers `ServerReferenceModule`s in the client module graph and builds
    /// their inner server modules as Node.js chunks.
    #[turbo_tasks::function]
    async fn server_reference_output_assets(
        self: Vc<Self>,
        asset_context: Vc<Box<dyn AssetContext>>,
        runtime_entries: Vc<EvaluatableAssets>,
    ) -> Result<Vc<OutputAssets>> {
        let this = self.await?;
        let project = *this.project;

        // Await all graphs simultaneously for better parallelization
        let resolved_graphs = this
            .entrypoints
            .iter()
            .map(|e| async {
                e.module_graph_for_entry(asset_context, runtime_entries)
                    .await
            })
            .try_join()
            .await?;

        // Walk all graphs to find ServerReferenceModule instances
        let mut unique_server_modules = turbo_tasks::FxIndexSet::default();
        for graph in &resolved_graphs {
            for module in graph.iter_nodes() {
                if let Some(server_ref) =
                    ResolvedVc::try_downcast_type::<ServerReferenceModule>(module)
                {
                    let inner = server_ref.await?;
                    unique_server_modules.insert(inner.server_module);
                }
            }
        }

        // Resolving VCs to strings for a deterministic sorting pass guarantees our
        // AST chunk hashes remain tightly identical between runs, following Next.js's
        // FxIndexMap/IndexSet pattern for chunking server routines.
        let mut pairs = unique_server_modules
            .into_iter()
            .map(|m| async move { Ok((m.ident().to_string().await?, m)) })
            .try_join()
            .await?;

        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        let server_modules: Vec<_> = pairs.into_iter().map(|(_, m)| m).collect();

        let server_function_assets: Vec<ResolvedVc<Box<dyn EvaluatableAsset>>> = server_modules
            .iter()
            .filter_map(|m| ResolvedVc::try_sidecast::<Box<dyn EvaluatableAsset>>(*m))
            .collect();

        let server_config = project.config().server().await?;
        let mut entry_specs = Vec::new();
        if let Some(entry) = &server_config.entry {
            match entry {
                ServerEntry::Import(import) => {
                    entry_specs.push((rcstr!("index"), Some(import.clone()), true, false));
                }
                ServerEntry::Entries(entries) => {
                    entry_specs.extend(entries.iter().enumerate().map(|(index, entry)| {
                        (
                            entry.name.clone(),
                            Some(entry.import.clone()),
                            index == 0,
                            true,
                        )
                    }));
                }
            }
        }
        if entry_specs.is_empty() && !server_function_assets.is_empty() {
            entry_specs.push((rcstr!("index"), None, true, false));
        }

        let mut entry_names = turbo_tasks::FxIndexSet::default();
        for (name, _, _, _) in &entry_specs {
            if !entry_names.insert(name.clone()) {
                bail!("duplicate server entry name `{name}`");
            }
        }

        let server_asset_context = if entry_specs.iter().any(|(_, import, _, _)| import.is_some()) {
            let server_layer =
                Layer::new_with_user_friendly_name(rcstr!("server"), rcstr!("Nodejs"));
            let server_compile_time_info = project.server_compile_time_info();
            let server_module_options_context = get_server_module_options_context(
                project.project_path().owned().await?,
                project.execution_context(),
                project.server_compile_time_info().environment(),
                project.mode(),
                project.config(),
            );
            let server_resolve_options_context = get_server_resolve_options_context(
                project.project_path().owned().await?,
                project.mode(),
                project.config(),
                project.execution_context(),
                project.pack_path().owned().await?,
            );
            Some(Vc::upcast(ModuleAssetContext::new(
                TransitionOptions::default().cell(),
                server_compile_time_info,
                server_module_options_context,
                server_resolve_options_context,
                server_layer,
            )))
        } else {
            None
        };

        let project_path = project.project_path().owned().await?;
        let mut build_entries = Vec::new();
        for (name, entry_import, include_server_functions, preserve_entry_name) in entry_specs {
            let mut evaluatable_assets = if include_server_functions {
                server_function_assets.clone()
            } else {
                Vec::new()
            };

            if let Some(entry_import) = entry_import {
                let relative_import =
                    convert_to_project_relative(&entry_import, &project_path.path)?;
                let entry_request = Request::relative(
                    relative_import.into(),
                    Default::default(),
                    Default::default(),
                    false,
                );
                let origin = PlainResolveOrigin::new(
                    server_asset_context.expect("server asset context is created for imports"),
                    project_path.join("_")?,
                )
                .await?;
                let resolve_options = origin.resolve_options();
                let asset_context = origin.asset_context();
                let origin_path = origin.origin_path();
                let ty = ReferenceType::Entry(EntryReferenceSubType::Undefined);
                let modules = asset_context
                    .resolve_asset(origin_path, entry_request, resolve_options, ty)
                    .await?
                    .primary_modules()
                    .await?;
                for &module in &*modules {
                    if let Some(entry) =
                        ResolvedVc::try_downcast::<Box<dyn EvaluatableAsset>>(module)
                    {
                        evaluatable_assets.push(entry);
                    }
                }
            }

            if evaluatable_assets.is_empty() {
                bail!("server entry `{name}` did not resolve to an evaluatable module");
            }
            build_entries.push((name, evaluatable_assets, preserve_entry_name));
        }

        if build_entries.is_empty() {
            return Ok(OutputAssets::empty());
        }

        // Build one graph for all server entries so shared modules can be identified across
        // independently emitted entry chunk groups.
        let entry_modules = build_entries
            .iter()
            .map(|(_, assets, _)| {
                assets
                    .iter()
                    .map(|entry| ResolvedVc::upcast(*entry))
                    .collect::<Vec<ResolvedVc<Box<dyn Module>>>>()
            })
            .collect::<Vec<_>>();
        let all_entry_modules = entry_modules.iter().flatten().copied().collect::<Vec<_>>();
        let initial_server_module_graph =
            project.server_fn_module_graph(Vc::cell(all_entry_modules.clone()));
        let initial_module_graph = initial_server_module_graph.await?;

        let mut module_usage = FxHashMap::default();
        for (entry_index, modules) in entry_modules.iter().enumerate() {
            initial_module_graph.traverse_nodes_dfs(
                modules.iter().copied(),
                &mut module_usage,
                |module, usage| {
                    usage
                        .entry(module)
                        .or_insert_with(Vec::new)
                        .push(entry_index);
                    Ok(GraphTraversalAction::Continue)
                },
                |_, _| Ok(()),
            )?;
        }

        let entry_module_set = all_entry_modules.iter().copied().collect::<FxHashSet<_>>();
        let mut shared_module_groups = FxHashMap::default();
        for (module, entry_indices) in module_usage {
            if entry_indices.len() <= 1 || entry_module_set.contains(&module) {
                continue;
            }
            if let Some(module) = ResolvedVc::try_sidecast::<Box<dyn ChunkableModule>>(module) {
                shared_module_groups
                    .entry(entry_indices)
                    .or_insert_with(Vec::new)
                    .push(module);
            }
        }
        let mut shared_module_groups = shared_module_groups.into_iter().collect::<Vec<_>>();
        // A shared module's dependencies are used by at least the same entries, so build wider
        // groups first and make them available to the narrower groups that depend on them.
        shared_module_groups.sort_by(|(a, _), (b, _)| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        let shared_modules = shared_module_groups
            .iter()
            .flat_map(|(_, modules)| modules.iter().copied())
            .collect::<Vec<_>>();

        // Shared modules are graph roots too, allowing their chunk group to be emitted first and
        // marked available while each entry chunk is constructed.
        let server_module_graph = if shared_modules.is_empty() {
            initial_server_module_graph
        } else {
            let mut graph_entries = all_entry_modules;
            graph_entries.extend(
                shared_modules
                    .iter()
                    .map(|module| ResolvedVc::upcast(*module)),
            );
            project.server_fn_module_graph(Vc::cell(graph_entries))
        };
        let server_chunking_context = project.server_fn_chunking_context();

        let mut entry_availability = AvailabilityInfo::root();
        let mut shared_assets_by_entry = vec![Vec::<Vc<OutputAssets>>::new(); build_entries.len()];
        for (entry_indices, shared_modules) in shared_module_groups {
            let shared_name: RcStr = if entry_indices.len() == build_entries.len() {
                rcstr!("server-shared")
            } else {
                format!(
                    "server-shared-{}",
                    entry_indices
                        .iter()
                        .map(usize::to_string)
                        .collect::<Vec<_>>()
                        .join("-")
                )
                .into()
            };
            let shared_ident =
                AssetIdent::from_path(project_path.join(&format!("{shared_name}.js"))?)
                    .with_query(format!("?name={shared_name}").into())
                    .into_vc();
            let shared_group = server_chunking_context
                .chunk_group(
                    shared_ident,
                    ChunkGroup::Entry(
                        shared_modules
                            .iter()
                            .map(|module| ResolvedVc::upcast(*module))
                            .collect(),
                    ),
                    server_module_graph,
                    entry_availability,
                )
                .await?;
            entry_availability = shared_group.availability_info;
            for entry_index in entry_indices {
                shared_assets_by_entry[entry_index].push(*shared_group.assets);
            }
        }

        let output_assets = build_entries
            .iter()
            .enumerate()
            .map(
                |(entry_index, (name, evaluatable_assets, preserve_entry_name))| {
                    let project_path = project_path.clone();
                    let shared_assets =
                        OutputAssets::concat(shared_assets_by_entry[entry_index].clone());
                    async move {
                        let modules = evaluatable_assets
                            .iter()
                            .map(|entry| ResolvedVc::upcast(*entry))
                            .collect();
                        let chunk_query = if *preserve_entry_name {
                            format!("?name={name}&preserveEntryName=1")
                        } else {
                            format!("?name={name}")
                        };
                        let ident =
                            AssetIdent::from_path(project_path.join(&format!("{name}.js"))?)
                                .with_query(chunk_query.into())
                                .into_vc();
                        let chunk_group_result = server_chunking_context
                            .evaluated_chunk_group(
                                ident,
                                ChunkGroup::Entry(modules),
                                server_module_graph,
                                shared_assets,
                                entry_availability,
                            )
                            .await?;
                        Ok(*chunk_group_result.assets)
                    }
                },
            )
            .try_join()
            .await?;

        Ok(OutputAssets::concat(output_assets))
    }
}
