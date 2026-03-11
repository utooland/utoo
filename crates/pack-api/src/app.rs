use anyhow::{Result, bail};
use pack_core::client::context::{
    get_client_module_options_context, get_client_resolve_options_context,
    get_client_runtime_entries,
};
use pack_core::config::Platform;
use pack_core::server::contexts::{
    get_server_module_options_context, get_server_resolve_options_context,
};
use pack_core::util::convert_to_project_relative;
use qstring::QString;
use tracing::Instrument;
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{Completion, JoinIterExt, ResolvedVc, TryJoinIterExt, ValueToString, Vc};
use turbo_tasks_fs::{File, FileContent};
use turbopack::{
    ModuleAssetContext, module_options::ModuleOptionsContext, transition::TransitionOptions,
};
use turbopack_core::chunk::ChunkingContextExt;
use turbopack_core::output::OutputAssetsWithReferenced;
use turbopack_core::resolve::origin::ResolveOrigin;
use turbopack_core::{
    asset::AssetContent,
    chunk::{
        ChunkingContext, EvaluatableAsset, EvaluatableAssets, availability_info::AvailabilityInfo,
    },
    context::AssetContext,
    ident::{AssetIdent, Layer},
    module::{Module, Modules},
    module_graph::{
        GraphEntries, ModuleGraph,
        chunk_group_info::{ChunkGroup, ChunkGroupEntry},
    },
    output::OutputAssets,
    reference_type::{EntryReferenceSubType, ReferenceType},
    resolve::{
        origin::{PlainResolveOrigin, ResolveOriginExt},
        parse::Request,
    },
    virtual_output::VirtualOutputAsset,
};

use crate::{
    endpoint::{Endpoint, EndpointOutput, EndpointOutputPaths},
    project::Project,
    webpack_stats::generate_webpack_stats,
};
use turbopack_resolve::resolve_options_context::ResolveOptionsContext;

#[turbo_tasks::value(transparent)]
pub struct AppEntripoints(pub Vec<AppEntrypoint>);

#[turbo_tasks::value]
pub struct AppProject {
    pub project: ResolvedVc<Project>,
    pub apps: ResolvedVc<AppEntripoints>,
}

#[turbo_tasks::value(transparent)]
pub struct OptionAppProject(Option<ResolvedVc<AppProject>>);

#[turbo_tasks::value_impl]
impl AppProject {
    #[turbo_tasks::function]
    pub fn new(project: ResolvedVc<Project>, apps: ResolvedVc<AppEntripoints>) -> Vc<Self> {
        Self { project, apps }.cell()
    }

    #[turbo_tasks::function]
    pub fn apps(&self) -> Vc<AppEntripoints> {
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
        );

        let ty = ReferenceType::Entry(EntryReferenceSubType::Undefined);

        Ok(origin
            .resolve_asset(entry_request, origin.resolve_options(), ty)
            .await?
            .primary_modules())
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

            let query = QString::new(vec![("name", this.name.as_str())]).to_string();
            let query = if query.is_empty() {
                // If name is empty, provide a default fallback
                QString::new(vec![("name", "index")]).to_string()
            } else {
                format!("?{query}")
            };

            let app_chunk_group = project
                .client_chunking_context()
                .evaluated_chunk_group_assets(
                    AssetIdent::from_path(
                        project.project_path().await?.join(this.import.as_str())?,
                    )
                    .with_query(query.into()),
                    ChunkGroup::Entry(
                        self.entry_evaluatable_assets(asset_context, runtime_entries)
                            .await?
                            .iter()
                            .map(|m| ResolvedVc::upcast(*m))
                            .collect(),
                    ),
                    module_graph,
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

            let dist_root = project.dist_root().owned().await?;
            let entry_filename = format!("{}.js", this.name);
            let entry_path = dist_root.join(&entry_filename)?;

            let app_chunk_group = project
                .server_chunking_context()
                .entry_chunk_group(
                    entry_path,
                    self.entry_evaluatable_assets(asset_context, runtime_entries),
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
        let platform = &*self.project().platform().await?;
        let chunk_group_assets = match platform {
            Platform::Web => {
                *self
                    .client_chunk_group(asset_context, runtime_entries)
                    .await?
                    .assets
            }
            Platform::Node => {
                *self
                    .server_chunk_group(asset_context, runtime_entries)
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
        match &*self.project().platform().await? {
            Platform::Web => {
                let watch = self.project().await?.watch.enable;
                Ok(get_client_runtime_entries(
                    self.project().project_path().owned().await?,
                    self.project().mode(),
                    self.project().config(),
                    self.project().execution_context(),
                    self.project().pack_path().owned().await?,
                    Vc::cell(watch),
                    Vc::cell(
                        watch
                            && self
                                .project()
                                .config()
                                .dev_server()
                                .await?
                                .hot
                                .unwrap_or_default(),
                    ),
                )
                .resolve_entries(Vc::upcast(self.app_module_context())))
            }
            Platform::Node => Ok(EvaluatableAssets::empty()),
        }
    }

    #[turbo_tasks::function]
    pub async fn app_module_context(self: Vc<Self>) -> Result<Vc<ModuleAssetContext>> {
        let platform = &*self.project().platform().await?;

        let compile_time_info = match platform {
            Platform::Web => self.project().client_compile_time_info(),
            Platform::Node => self.project().server_compile_time_info(),
        };

        let layer = match platform {
            Platform::Web => {
                Layer::new_with_user_friendly_name(rcstr!("client"), rcstr!("Browser"))
            }
            Platform::Node => {
                Layer::new_with_user_friendly_name(rcstr!("server"), rcstr!("Nodejs"))
            }
        };

        Ok(ModuleAssetContext::new(
            // FIXME:
            TransitionOptions {
                ..Default::default()
            }
            .cell(),
            compile_time_info,
            self.app_module_options_context(),
            self.app_resolve_options_context(),
            layer,
        ))
    }

    #[turbo_tasks::function]
    async fn app_module_options_context(self: Vc<Self>) -> Result<Vc<ModuleOptionsContext>> {
        match &*self.project().platform().await? {
            Platform::Web => Ok(get_client_module_options_context(
                self.project().project_path().owned().await?,
                self.project().execution_context(),
                self.project().client_compile_time_info().environment(),
                self.project().mode(),
                self.project().config(),
                Vc::cell(self.project().await?.watch.enable),
                self.project().pack_path().owned().await?,
            )),
            Platform::Node => Ok(get_server_module_options_context(
                self.project().project_path().owned().await?,
                self.project().execution_context(),
                self.project().server_compile_time_info().environment(),
                self.project().mode(),
                self.project().config(),
            )),
        }
    }

    #[turbo_tasks::function]
    async fn app_resolve_options_context(self: Vc<Self>) -> Result<Vc<ResolveOptionsContext>> {
        match &*self.project().platform().await? {
            Platform::Web => Ok(get_client_resolve_options_context(
                self.project().project_path().owned().await?,
                self.project().mode(),
                self.project().config(),
                self.project().execution_context(),
                self.project().pack_path().owned().await?,
            )),
            Platform::Node => Ok(get_server_resolve_options_context(
                self.project().project_path().owned().await?,
                self.project().mode(),
                self.project().config(),
                self.project().execution_context(),
                self.project().pack_path().owned().await?,
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
                let evaluatable_assets =
                    e.entry_evaluatable_assets(Vc::upcast(asset_context), runtime_entries);
                let entry_modules: Vec<ResolvedVc<Box<dyn Module>>> = evaluatable_assets
                    .await?
                    .iter()
                    .copied()
                    .map(ResolvedVc::upcast)
                    .collect();

                Ok(ChunkGroupEntry::Entry(entry_modules))
            })
            .try_join()
            .await?;

        Ok(Vc::cell(entries))
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

            let dist_root = this.project.dist_root().await?;

            let written_endpoint = EndpointOutputPaths::NodeJs {
                server_entry_path: dist_root.path.clone(),
                // TODO: set right server path when server rendering supported
                server_paths: vec![],
                client_paths: vec![],
            };

            let should_create_webpack_stats = *this.project.should_create_webpack_stats().await?;

            let output_assets = if !should_create_webpack_stats {
                output_assets
            } else {
                let webpack_stats = generate_webpack_stats(output_assets, this.project.dist_root());
                let webpack_stats_read = webpack_stats.await?;
                let dist_root_owned = this.project.dist_root().owned().await?;
                let stats_json = simd_json::serde::to_string(&*webpack_stats_read)?;
                let stats_output = VirtualOutputAsset::new(
                    dist_root_owned.join("stats.json")?,
                    AssetContent::file(FileContent::from(File::from(stats_json)).cell()),
                )
                .to_resolved()
                .await?;
                output_assets.concatenate(*ResolvedVc::cell(vec![ResolvedVc::upcast(stats_output)]))
            };

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
    fn server_changed(self: Vc<Self>) -> Vc<Completion> {
        Completion::new()
    }

    #[turbo_tasks::function]
    fn client_changed(self: Vc<Self>) -> Vc<Completion> {
        Completion::new()
    }
}
