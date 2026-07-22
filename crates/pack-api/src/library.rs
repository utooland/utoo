use anyhow::{Result, bail};
use pack_core::client::context::{
    get_client_module_options_context, get_client_resolve_options_context,
    get_client_runtime_entries,
};
use pack_core::config::Platform;

use pack_core::library::contexts::{LibraryChunkingContextOptions, get_library_chunking_context};
use pack_core::server::contexts::{
    get_server_module_options_context, get_server_resolve_options_context,
};
use pack_core::util::convert_to_project_relative;
use tracing::{Instrument, trace_span};
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{Completion, JoinIterExt, ResolvedVc, ValueToString, Vc};
use turbopack::{
    ModuleAssetContext, module_options::ModuleOptionsContext, transition::TransitionOptions,
};
use turbopack_core::{
    chunk::{
        ChunkGroupResult, ChunkingContext, EvaluatableAsset, EvaluatableAssets,
        availability_info::AvailabilityInfo,
    },
    context::AssetContext,
    ident::{AssetIdent, Layer},
    module::{Module, Modules},
    module_graph::{
        GraphEntries, ModuleGraph,
        chunk_group_info::{ChunkGroup, ChunkGroupEntry, EntryHeuristics},
    },
    output::OutputAssets,
    reference_type::{EntryReferenceSubType, ReferenceType},
    resolve::{
        origin::{PlainResolveOrigin, ResolveOrigin},
        parse::Request,
    },
};
use turbopack_resolve::resolve_options_context::ResolveOptionsContext;

use crate::{
    endpoint::{Endpoint, EndpointOutput, EndpointOutputPaths},
    paths::initial_paths_in_root,
    project::Project,
};

#[turbo_tasks::value]
pub struct LibraryEntrypoint {
    pub name: RcStr,
    pub import: RcStr,
    pub runtime_root: Option<RcStr>,
    pub runtime_export: Option<Vec<RcStr>>,
}

#[turbo_tasks::value(transparent)]
pub struct LibraryEntrypoints(pub Vec<LibraryEntrypoint>);

#[turbo_tasks::value]
pub struct LibraryProject {
    pub project: ResolvedVc<Project>,
    pub libraries: ResolvedVc<LibraryEntrypoints>,
}

#[turbo_tasks::value(transparent)]
pub struct OptionLibraryProject(Option<ResolvedVc<LibraryProject>>);

#[turbo_tasks::value_impl]
impl LibraryProject {
    #[turbo_tasks::function]
    pub fn new(
        project: ResolvedVc<Project>,
        libraries: ResolvedVc<LibraryEntrypoints>,
    ) -> Vc<Self> {
        Self { project, libraries }.cell()
    }

    #[turbo_tasks::function]
    pub fn libraries(&self) -> Vc<LibraryEntrypoints> {
        *self.libraries
    }

    #[turbo_tasks::function]
    pub async fn get_library_endpoints(self: Vc<Self>) -> Result<Vc<LibraryEndpoints>> {
        let this = self.await?;

        let project = this.project;

        let endpoints = this
            .libraries
            .await?
            .iter()
            .map(|l| async move {
                LibraryEndpoint {
                    project,
                    name: l.name.clone(),
                    import: l.import.clone(),
                    runtime_root: l.runtime_root.clone(),
                    runtime_export: l.runtime_export.clone().unwrap_or_default(),
                }
                .resolved_cell()
            })
            .join()
            .await;

        Ok(LibraryEndpoints(endpoints).cell())
    }
}

#[turbo_tasks::value]
pub struct LibraryEndpoint {
    project: ResolvedVc<Project>,
    name: RcStr,
    import: RcStr,
    runtime_root: Option<RcStr>,
    runtime_export: Vec<RcStr>,
}

#[turbo_tasks::value(transparent)]
pub struct LibraryEndpoints(pub Vec<ResolvedVc<LibraryEndpoint>>);

#[turbo_tasks::value_impl]
impl LibraryEndpoint {
    #[turbo_tasks::function]
    fn project(&self) -> Vc<Project> {
        *self.project
    }

    #[turbo_tasks::function]
    async fn library_module_context(self: Vc<Self>) -> Result<Vc<ModuleAssetContext>> {
        let project = self.project();
        let platform = &*project.platform().await?;

        let layer = match platform {
            Platform::Node => Layer::new_with_user_friendly_name(
                rcstr!("library-server"),
                rcstr!("Library (Node)"),
            ),
            Platform::Web => Layer::new_with_user_friendly_name(
                rcstr!("library-client"),
                rcstr!("Library (Web)"),
            ),
        };

        Ok(ModuleAssetContext::new(
            // FIXME:
            TransitionOptions {
                ..Default::default()
            }
            .cell(),
            project.compile_time_info_for_platform(),
            self.library_module_options_context(),
            self.library_resolve_options_context(),
            layer,
        ))
    }

    #[turbo_tasks::function]
    async fn library_module_options_context(self: Vc<Self>) -> Result<Vc<ModuleOptionsContext>> {
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
                Vc::cell(false),
                project.pack_path().owned().await?,
            )),
        }
    }

    #[turbo_tasks::function]
    async fn library_resolve_options_context(self: Vc<Self>) -> Result<Vc<ResolveOptionsContext>> {
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

    #[turbo_tasks::function]
    async fn library_runtime_entries(self: Vc<Self>) -> Result<Vc<EvaluatableAssets>> {
        let project = self.project();
        match &*project.platform().await? {
            Platform::Node => Ok(EvaluatableAssets::empty()),
            Platform::Web => {
                Ok(get_client_runtime_entries(
                    project.project_path().owned().await?,
                    project.mode(),
                    project.config(),
                    project.execution_context(),
                    project.pack_path().owned().await?,
                    // Library project not support watch mode
                    Vc::cell(false),
                    Vc::cell(false),
                )
                .resolve_entries(Vc::upcast(self.library_module_context())))
            }
        }
    }

    #[turbo_tasks::function]
    pub async fn library_entry_modules(self: Vc<Self>) -> Result<Vc<Modules>> {
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

        let asset_context = Vc::upcast(self.library_module_context());
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
    async fn library_evaluatable_assets(self: Vc<Self>) -> Result<Vc<EvaluatableAssets>> {
        let modules = self.library_entry_modules().await?;

        let mut runtime_entries = Vec::with_capacity(modules.len());
        for &module in &modules {
            if let Some(entry) = ResolvedVc::try_downcast::<Box<dyn EvaluatableAsset>>(module) {
                runtime_entries.push(*entry);
            } else {
                bail!(
                    "runtime reference resolved to an asset ({}) that cannot be evaluated",
                    module.ident().to_string().await?
                );
            }
        }

        runtime_entries.extend(self.library_runtime_entries().await?.iter().map(|e| **e));

        Ok(EvaluatableAssets::many(runtime_entries))
    }

    #[turbo_tasks::function]
    async fn library_module_graph(self: Vc<Self>) -> Result<Vc<ModuleGraph>> {
        let project = self.project();
        let evaluatable_assets = self.library_evaluatable_assets();
        Ok(project.module_graph_for_modules(evaluatable_assets))
    }

    #[turbo_tasks::function]
    pub(super) async fn library_chunking_context(
        self: Vc<Self>,
        runtime_root: Vc<Option<RcStr>>,
        runtime_export: Vc<Vec<RcStr>>,
    ) -> Result<Vc<Box<dyn ChunkingContext>>> {
        let project = self.project();
        let output_root_to_root_path = project.node_root_to_root_path().await?;

        Ok(get_library_chunking_context(
            LibraryChunkingContextOptions {
                name: Vc::cell(Some(self.await?.name.clone())),
                preserve_entry_name: false,
                shared_chunks: false,
                filename_override: None,
                mode: project.mode(),
                root_path: project.project_path().owned().await?,
                output_root: project.dist_root().owned().await?,
                output_root_to_root_path: (*output_root_to_root_path).clone(),
                environment: project.client_compile_time_info().environment(),
                module_id_strategy: project.module_ids(),
                no_mangling: project.no_mangling(),
                compress: project.compress(),
                runtime_root,
                runtime_export,
                config: project.config(),
                export_usage: project.export_usage(),
                unused_references: project.unused_references(),
                platform: project.platform(),
            },
        ))
    }

    #[turbo_tasks::function]
    async fn library_chunk(self: Vc<Self>) -> Result<Vc<ChunkGroupResult>> {
        async move {
            let this = self.await?;

            let project = self.project();

            let library_chunking_context = self.library_chunking_context(
                Vc::cell(this.runtime_root.clone()),
                Vc::cell(this.runtime_export.clone()),
            );

            let module_graph = self.library_module_graph();

            let query = format!("?name={}", this.name);

            let library_chunk_group = library_chunking_context.evaluated_chunk_group(
                AssetIdent::from_path(project.project_path().await?.join(this.import.as_str())?)
                    .with_query(query.into())
                    .into_vc(),
                ChunkGroup::Entry(self.library_entry_modules().await?.to_vec()),
                module_graph,
                OutputAssets::empty(),
                AvailabilityInfo::root(),
            );

            Ok(library_chunk_group)
        }
        .instrument(tracing::trace_span!("library chunk rendering"))
        .await
    }

    #[turbo_tasks::function]
    pub async fn output_assets(self: Vc<Self>) -> Result<Vc<OutputAssets>> {
        let chunk_group_assets = *self.library_chunk().await?.assets;
        Ok(chunk_group_assets)
    }
}

#[turbo_tasks::value_impl]
impl Endpoint for LibraryEndpoint {
    #[turbo_tasks::function]
    async fn entries(self: Vc<Self>) -> Result<Vc<GraphEntries>> {
        let mut entry_modules: Vec<ResolvedVc<Box<dyn Module>>> = self
            .library_runtime_entries()
            .await?
            .iter()
            .copied()
            .map(ResolvedVc::upcast)
            .collect();
        entry_modules.extend(self.library_entry_modules().await?);
        Ok(
            GraphEntries::from_chunk_groups(vec![ChunkGroupEntry::Entry {
                modules: entry_modules,
                heuristics: EntryHeuristics::default(),
            }])
            .cell(),
        )
    }

    #[turbo_tasks::function]
    async fn output(self: Vc<Self>) -> Result<Vc<EndpointOutput>> {
        let span = trace_span!("library endpoint");
        async move {
            let this = self.await?;
            let output_assets = self.output_assets();
            let output_assets = output_assets.concatenate(self.project().copy_output_assets());

            let dist_root_vc = self.project().dist_root();
            let dist_root = dist_root_vc.await?;
            let client_paths = initial_paths_in_root(output_assets, dist_root_vc)
                .await?
                .iter()
                .cloned()
                .collect();

            let written_endpoint = EndpointOutputPaths::NodeJs {
                // FIXME: No server path when bundling library
                server_entry_path: dist_root.path.clone(),
                server_paths: vec![],
                client_paths,
            };

            Ok(EndpointOutput {
                output_assets: output_assets.to_resolved().await?,
                output_paths: written_endpoint.resolved_cell(),
                project: this.project,
            }
            .cell())
        }
        .instrument(span)
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
