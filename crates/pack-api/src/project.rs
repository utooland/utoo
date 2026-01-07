use anyhow::{Context, Result, bail};
use pack_core::{
    client::context::{
        ClientChunkingContextOptions, get_client_chunking_context, get_client_compile_time_info,
    },
    config::{Config, ModuleIds as ModuleIdStrategyConfig},
    emit_assets,
    mode::Mode,
    util::{Runtime, convert_to_project_relative},
};
use serde::Deserialize;
use std::{
    fs,
    path::{MAIN_SEPARATOR, Path, PathBuf},
    time::Duration,
};
use tracing::Instrument;
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{
    Completion, Completions, IntoTraitRef, OperationValue, OperationVc, ReadRef, ResolvedVc, State,
    TransientInstance, TryFlatJoinIterExt, TryJoinIterExt, Vc, mark_root,
};
use turbo_tasks_env::{EnvMap, ProcessEnv};
use turbo_tasks_fs::{
    DirectoryContent, DirectoryEntry, DiskFileSystem, FileContent, FileSystem, FileSystemEntryType,
    FileSystemPath, VirtualFileSystem, invalidation,
};
use turbo_unix_path::normalize_path;
use turbopack::global_module_ids::get_global_module_id_strategy;
use turbopack_core::{
    file_source::FileSource,
    module_graph::binding_usage_info::{
        BindingUsageInfo, OptionBindingUsageInfo, compute_binding_usage_info,
    },
};

use turbopack::evaluate_context::node_build_environment;

use turbopack_core::{
    PROJECT_FILESYSTEM_NAME,
    changed::content_changed,
    chunk::{
        ChunkingContext, EvaluatableAssets, SourceMapsType,
        module_id_strategies::{DevModuleIdStrategy, ModuleIdStrategy},
    },
    compile_time_info::CompileTimeInfo,
    issue::{
        CollectibleIssuesExt, Issue, IssueSeverity, IssueStage, OptionStyledString, StyledString,
    },
    module_graph::{
        GraphEntries, ModuleGraph, SingleModuleGraph, VisitedModules,
        chunk_group_info::ChunkGroupEntry,
    },
    output::{
        ExpandOutputAssetsInput, ExpandedOutputAssets, OutputAsset, OutputAssets,
        expand_output_assets,
    },
    raw_output::RawOutput,
    reference::all_assets_from_entries,
    version::{
        NotFoundVersion, OptionVersionedContent, Update, Version, VersionState, VersionedContent,
    },
};
use turbopack_node::execution_context::ExecutionContext;
use turbopack_nodejs::NodeJsChunkingContext;

use crate::{
    app::{AppEntrypoint, AppProject, OptionAppProject},
    endpoint::{Endpoint, Endpoints},
    entrypoint::Entrypoints,
    library::{LibraryEntrypoint, LibraryProject, OptionLibraryProject},
    versioned_content_map::VersionedContentMap,
};

#[turbo_tasks::value]
#[derive(Default, Debug, Clone, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct WatchOptions {
    /// Whether to watch the filesystem for file changes.
    pub enable: bool,

    /// Enable polling at a certain interval if the native file watching doesn't work (e.g.
    /// docker).
    pub poll_interval: Option<Duration>,
}

#[turbo_tasks::value]
#[derive(Debug, Clone, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOptions {
    /// A root path from which all files must be nested under. Trying to access
    /// a file outside this root will fail. Think of this as a chroot.
    pub root_path: RcStr,

    /// A path inside the root_path which contains the app directories.
    pub project_path: RcStr,

    /// The contents of bundler config, serialized to JSON.
    pub config: RcStr,

    /// A map of environment variables to use when compiling code.
    pub process_env: Vec<(RcStr, RcStr)>,

    /// A map of environment variables which should get injected at compile
    /// time.
    pub define_env: DefineEnv,

    /// Filesystem watcher options.
    pub watch: WatchOptions,

    /// The mode in which Next.js is running.
    pub dev: bool,

    /// The build id.
    pub build_id: RcStr,

    /// Absolute path for `@utoo/pack`.
    pub pack_path: RcStr,
}

#[turbo_tasks::value]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialProjectOptions {
    /// A root path from which all files must be nested under. Trying to access
    /// a file outside this root will fail. Think of this as a chroot.
    pub root_path: Option<RcStr>,

    /// A path inside the root_path which contains the app/pages directories.
    pub project_path: Option<RcStr>,

    /// The contents of next.config.js, serialized to JSON.
    pub config: Option<RcStr>,

    /// A map of environment variables to use when compiling code.
    pub process_env: Option<Vec<(RcStr, RcStr)>>,

    /// A map of environment variables which should get injected at compile
    /// time.
    pub define_env: Option<DefineEnv>,

    /// Filesystem watcher options.
    pub watch: Option<WatchOptions>,

    /// The build id.
    pub build_id: Option<RcStr>,

    /// Absolute path for `@utoo/pack`.
    pub pack_path: Option<RcStr>,
}

#[turbo_tasks::value]
#[derive(Default, Debug, Clone, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct DefineEnv {
    pub client: Vec<(RcStr, RcStr)>,
    pub edge: Vec<(RcStr, RcStr)>,
    pub nodejs: Vec<(RcStr, RcStr)>,
}

#[turbo_tasks::value]
pub struct ProjectContainer {
    name: RcStr,
    options_state: State<Option<ProjectOptions>>,
    versioned_content_map: Option<ResolvedVc<VersionedContentMap>>,
}

#[turbo_tasks::value_impl]
impl ProjectContainer {
    #[turbo_tasks::function]
    pub async fn new(name: RcStr, dev: bool) -> Result<Vc<Self>> {
        Ok(ProjectContainer {
            name,
            // we only need to enable versioning in dev mode, since build
            // is assumed to be operating over a static snapshot
            versioned_content_map: if dev {
                Some(VersionedContentMap::new())
            } else {
                None
            },
            options_state: State::new(None),
        }
        .cell())
    }
}

#[turbo_tasks::function(operation)]
fn project_fs_operation(project: ResolvedVc<Project>) -> Vc<DiskFileSystem> {
    project.project_fs(project.dist_dir())
}

#[turbo_tasks::function(operation)]
fn output_fs_operation(project: ResolvedVc<Project>) -> Vc<DiskFileSystem> {
    project.output_fs()
}

impl ProjectContainer {
    #[tracing::instrument(level = "trace", name = "initialize project", skip_all)]
    pub async fn initialize(self: ResolvedVc<Self>, options: ProjectOptions) -> Result<()> {
        let watch = options.watch.clone();

        self.await?.options_state.set(Some(options));

        let project = self.project().to_resolved().await?;
        let project_fs = project_fs_operation(project)
            .read_strongly_consistent()
            .await?;
        if watch.enable {
            project_fs
                .start_watching_with_invalidation_reason(watch.poll_interval)
                .await?;
        } else {
            project_fs.invalidate_with_reason(|path| invalidation::Initialize {
                path: RcStr::from(path.to_string_lossy()),
            });
        }
        let output_fs = output_fs_operation(project)
            .read_strongly_consistent()
            .await?;
        output_fs.invalidate_with_reason(|path| invalidation::Initialize {
            path: RcStr::from(path.to_string_lossy()),
        });
        Ok(())
    }

    #[tracing::instrument(level = "trace", name = "update project", skip_all)]
    pub async fn update(self: Vc<Self>, options: PartialProjectOptions) -> Result<()> {
        let PartialProjectOptions {
            root_path,
            project_path,
            config,
            process_env,
            define_env,
            watch,
            build_id,
            pack_path,
        } = options;

        let this = self.await?;

        let mut new_options = this
            .options_state
            .get()
            .clone()
            .context("ProjectContainer need to be initialized with initialize()")?;

        if let Some(root_path) = root_path {
            new_options.root_path = root_path;
        }
        if let Some(project_path) = project_path {
            new_options.project_path = project_path;
        }
        if let Some(config) = config {
            new_options.config = config;
        }
        if let Some(process_env) = process_env {
            new_options.process_env = process_env;
        }
        if let Some(define_env) = define_env {
            new_options.define_env = define_env;
        }
        if let Some(watch) = watch {
            new_options.watch = watch;
        }

        if let Some(build_id) = build_id {
            new_options.build_id = build_id;
        }

        if let Some(pack_path) = pack_path {
            new_options.pack_path = pack_path;
        }

        // TODO: Handle mode switch, should prevent mode being switched.
        let watch = new_options.watch.clone();

        let project = self.project().to_resolved().await?;
        let prev_project_fs = project_fs_operation(project)
            .read_strongly_consistent()
            .await?;
        let prev_output_fs = output_fs_operation(project)
            .read_strongly_consistent()
            .await?;

        this.options_state.set(Some(new_options));
        let project = self.project().to_resolved().await?;
        let project_fs = project_fs_operation(project)
            .read_strongly_consistent()
            .await?;
        let output_fs = output_fs_operation(project)
            .read_strongly_consistent()
            .await?;

        if !ReadRef::ptr_eq(&prev_project_fs, &project_fs) {
            if watch.enable {
                // TODO stop watching: prev_project_fs.stop_watching()?;
                project_fs
                    .start_watching_with_invalidation_reason(watch.poll_interval)
                    .await?;
            } else {
                project_fs.invalidate_with_reason(|path| invalidation::Initialize {
                    // this path is just used for display purposes
                    path: RcStr::from(path.to_string_lossy()),
                });
            }
        }
        if !ReadRef::ptr_eq(&prev_output_fs, &output_fs) {
            prev_output_fs.invalidate_with_reason(|path| invalidation::Initialize {
                path: RcStr::from(path.to_string_lossy()),
            });
        }

        Ok(())
    }
}

#[turbo_tasks::value_impl]
impl ProjectContainer {
    #[turbo_tasks::function]
    pub async fn project(&self) -> Result<Vc<Project>> {
        let env_map: Vc<EnvMap>;
        let config;
        let define_env;
        let root_path;
        let project_path;
        let watch;
        let build_id;
        let pack_path;
        {
            let options = self.options_state.get();
            let options = options
                .as_ref()
                .context("ProjectContainer need to be initialized with initialize()")?;

            env_map = Vc::cell(options.process_env.iter().cloned().collect());
            define_env = ProjectDefineEnv {
                client: ResolvedVc::cell(options.define_env.client.iter().cloned().collect()),
                edge: ResolvedVc::cell(options.define_env.edge.iter().cloned().collect()),
                nodejs: ResolvedVc::cell(options.define_env.nodejs.iter().cloned().collect()),
            }
            .cell();
            config = Config::from_string(Vc::cell(options.config.clone()));
            root_path = options.root_path.clone();
            project_path = options.project_path.clone();
            watch = options.watch.clone();
            build_id = options.build_id.clone();
            pack_path = options.pack_path.clone();
        }

        Ok(Project {
            root_path,
            project_path,
            watch,
            config: config.to_resolved().await?,
            process_env: ResolvedVc::upcast(env_map.to_resolved().await?),
            define_env: define_env.to_resolved().await?,
            versioned_content_map: self.versioned_content_map,
            build_id,
            pack_path,
        }
        .cell())
    }

    /// See [Project::entrypoints].
    #[turbo_tasks::function]
    pub fn entrypoints(self: Vc<Self>) -> Vc<Entrypoints> {
        self.project().entrypoints()
    }

    /// See [Project::hmr_identifiers].
    #[turbo_tasks::function]
    pub fn hmr_identifiers(self: Vc<Self>) -> Vc<Vec<RcStr>> {
        self.project().hmr_identifiers()
    }

    /// Gets a source map for a particular `file_path`. If `dev` mode is disabled, this will always
    /// return [`OptionStringifiedSourceMap::none`].
    #[turbo_tasks::function]
    pub fn get_source_map(
        &self,
        file_path: FileSystemPath,
        section: Option<RcStr>,
    ) -> Vc<FileContent> {
        if let Some(map) = self.versioned_content_map {
            map.get_source_map(file_path, section)
        } else {
            FileContent::NotFound.cell()
        }
    }
}

#[turbo_tasks::value]
pub struct Project {
    /// A root path from which all files must be nested under. Trying to access
    /// a file outside this root will fail. Think of this as a chroot.
    root_path: RcStr,

    /// A path inside the root_path which contains the app/pages directories.
    pub project_path: RcStr,

    /// Filesystem watcher options.
    pub watch: WatchOptions,

    /// Config.
    config: ResolvedVc<Config>,

    /// A map of environment variables to use when compiling code.
    process_env: ResolvedVc<Box<dyn ProcessEnv>>,

    /// A map of environment variables which should get injected at compile
    /// time.
    define_env: ResolvedVc<ProjectDefineEnv>,

    versioned_content_map: Option<ResolvedVc<VersionedContentMap>>,

    build_id: RcStr,

    /// Absolute path for `@utoo/pack`.
    pack_path: RcStr,
}

// TODO: This may be not needed.
#[turbo_tasks::value]
pub struct ProjectDefineEnv {
    client: ResolvedVc<EnvMap>,
    edge: ResolvedVc<EnvMap>,
    nodejs: ResolvedVc<EnvMap>,
}

#[turbo_tasks::value_impl]
impl ProjectDefineEnv {
    #[turbo_tasks::function]
    pub fn client(&self) -> Vc<EnvMap> {
        *self.client
    }

    #[turbo_tasks::function]
    pub fn edge(&self) -> Vc<EnvMap> {
        *self.edge
    }

    #[turbo_tasks::function]
    pub fn nodejs(&self) -> Vc<EnvMap> {
        *self.nodejs
    }
}

#[turbo_tasks::value(shared)]
struct ConflictIssue {
    path: FileSystemPath,
    title: ResolvedVc<StyledString>,
    description: ResolvedVc<StyledString>,
    severity: IssueSeverity,
}

#[turbo_tasks::value_impl]
impl Issue for ConflictIssue {
    #[turbo_tasks::function]
    fn stage(&self) -> Vc<IssueStage> {
        IssueStage::AppStructure.cell()
    }

    fn severity(&self) -> IssueSeverity {
        self.severity
    }

    #[turbo_tasks::function]
    fn file_path(&self) -> Vc<FileSystemPath> {
        self.path.clone().cell()
    }

    #[turbo_tasks::function]
    fn title(&self) -> Vc<StyledString> {
        *self.title
    }

    #[turbo_tasks::function]
    fn description(&self) -> Vc<OptionStyledString> {
        Vc::cell(Some(self.description))
    }
}

#[turbo_tasks::value(transparent)]
pub struct OutputAssetVec(pub Vec<ResolvedVc<Box<dyn OutputAsset>>>);

#[turbo_tasks::value_impl]
impl Project {
    #[turbo_tasks::function]
    pub async fn library_project(self: Vc<Self>) -> Result<Vc<OptionLibraryProject>> {
        let this = self.await?;
        let lib_vec: Vec<LibraryEntrypoint> = this
            .config
            .entries()
            .await?
            .iter()
            .filter_map(|e| {
                e.library.as_ref().map(|l| LibraryEntrypoint {
                    name: e.name.clone().unwrap_or(
                        PathBuf::from(e.import.as_str())
                            .file_stem()
                            .unwrap()
                            .to_string_lossy()
                            .into(),
                    ),
                    import: e.import.clone(),
                    runtime_root: l.name.clone(),
                    runtime_export: l.export.clone(),
                })
            })
            .collect();
        if lib_vec.is_empty() {
            Ok(Vc::cell(None))
        } else {
            Ok(Vc::cell(Some(
                LibraryProject::new(self, Vc::cell(lib_vec))
                    .to_resolved()
                    .await?,
            )))
        }
    }

    #[turbo_tasks::function]
    pub async fn app_project(self: Vc<Self>) -> Result<Vc<OptionAppProject>> {
        let this = self.await?;
        let app_entrypoints: Vec<AppEntrypoint> = this
            .config
            .entries()
            .await?
            .iter()
            .filter_map(|e| {
                e.library.as_ref().map_or_else(
                    || {
                        Some(async {
                            Ok(AppEntrypoint {
                                project: self.to_resolved().await?,
                                name: e.name.clone().unwrap_or(
                                    PathBuf::from(e.import.as_str())
                                        .file_stem()
                                        .unwrap()
                                        .to_string_lossy()
                                        .into(),
                                ),
                                import: e.import.clone(),
                            })
                        })
                    },
                    |_| None,
                )
            })
            .try_join()
            .await?;
        if app_entrypoints.is_empty() {
            Ok(Vc::cell(None))
        } else {
            Ok(Vc::cell(Some(
                AppProject::new(self, Vc::cell(app_entrypoints))
                    .to_resolved()
                    .await?,
            )))
        }
    }

    #[turbo_tasks::function]
    pub async fn project_fs(&self, denied_path: Vc<RcStr>) -> Result<Vc<DiskFileSystem>> {
        let mut denied_paths = Vec::new();

        let denied_path = denied_path.await?;
        if !denied_path.is_empty() {
            let normalized =
                normalize_path(&denied_path).map_or_else(|| (*denied_path).clone(), RcStr::from);
            if !normalized.is_empty() {
                denied_paths.push(normalized);
            }
        }

        if self.pack_path.starts_with(&*self.root_path) {
            let relative_pack_path = self.pack_path.strip_prefix(&*self.root_path).unwrap();
            let relative_pack_path = relative_pack_path.trim_start_matches(MAIN_SEPARATOR);
            let turbopack_path = Path::new(relative_pack_path).join(".turbopack");
            if let Some(path_str) = turbopack_path.to_str() {
                let turbopack_path_normalized =
                    normalize_path(path_str).map_or_else(|| path_str.into(), RcStr::from);
                if !turbopack_path_normalized.is_empty()
                    && !denied_paths.contains(&turbopack_path_normalized)
                {
                    denied_paths.push(turbopack_path_normalized);
                }
            }
        }

        if denied_paths.is_empty() {
            Ok(DiskFileSystem::new(
                PROJECT_FILESYSTEM_NAME.into(),
                self.root_path.clone(),
            ))
        } else {
            Ok(DiskFileSystem::new_with_denied_paths(
                PROJECT_FILESYSTEM_NAME.into(),
                self.root_path.clone(),
                denied_paths,
            ))
        }
    }

    #[turbo_tasks::function]
    pub fn client_fs(self: Vc<Self>) -> Vc<Box<dyn FileSystem>> {
        let virtual_fs = VirtualFileSystem::new_with_name("client-fs".into());
        Vc::upcast(virtual_fs)
    }

    #[turbo_tasks::function]
    pub fn output_fs(&self) -> Vc<DiskFileSystem> {
        DiskFileSystem::new(rcstr!("output"), self.root_path.clone())
    }

    #[turbo_tasks::function]
    pub async fn project_root(self: Vc<Self>) -> Result<Vc<FileSystemPath>> {
        Ok(self.project_fs(self.dist_dir()).root())
    }

    #[turbo_tasks::function]
    pub async fn dist_dir(self: Vc<Self>) -> Result<Vc<RcStr>> {
        let this = self.await?;
        let dist_path = this
            .config
            .output()
            .await?
            .path
            .clone()
            .unwrap_or("dist".into());

        let relative_dist_path = convert_to_project_relative(&dist_path, &this.project_path)?;

        Ok(Vc::cell(relative_dist_path))
    }

    #[turbo_tasks::function]
    pub async fn node_root(self: Vc<Self>) -> Result<Vc<FileSystemPath>> {
        let this = self.await?;
        let pack_relative = if this.pack_path.starts_with(&*this.root_path) {
            this.pack_path.strip_prefix(&*this.root_path).unwrap()
        } else {
            this.pack_path.as_str()
        };
        let pack_relative = pack_relative
            .strip_prefix(MAIN_SEPARATOR)
            .unwrap_or(pack_relative)
            .replace(MAIN_SEPARATOR, "/");

        Ok(self
            .output_fs()
            .root()
            .await?
            .join(&pack_relative)?
            .join(".turbopack")?
            .cell())
    }

    #[turbo_tasks::function]
    pub async fn dist_root(self: Vc<Self>) -> Result<Vc<FileSystemPath>> {
        let this = self.await?;
        let dist_dir = self.dist_dir().await?;

        let project_relative = this.project_path.strip_prefix(&*this.root_path).unwrap();
        let project_relative = project_relative
            .strip_prefix(MAIN_SEPARATOR)
            .unwrap_or(project_relative)
            .replace(MAIN_SEPARATOR, "/");

        Ok(self
            .output_fs()
            .root()
            .await?
            .join(&project_relative)?
            .join(dist_dir.as_str())?
            .cell())
    }

    #[turbo_tasks::function]
    pub fn client_root(self: Vc<Self>) -> Vc<FileSystemPath> {
        self.client_fs().root()
    }

    #[turbo_tasks::function]
    pub async fn node_root_to_root_path(self: Vc<Self>) -> Result<Vc<RcStr>> {
        let node_root = self.node_root().owned().await?;
        let output_root = self.output_fs().root().owned().await?;
        let output_root_to_root_path = node_root
            .get_relative_path_to(&output_root)
            .context("Pack path need to be in root path")?;
        Ok(Vc::cell(output_root_to_root_path))
    }

    #[turbo_tasks::function]
    pub async fn project_path(self: Vc<Self>) -> Result<Vc<FileSystemPath>> {
        let this = self.await?;
        let root = self.project_root().await?;
        let project_relative = this.project_path.strip_prefix(&*this.root_path).unwrap();
        let project_relative = project_relative
            .strip_prefix(MAIN_SEPARATOR)
            .unwrap_or(project_relative)
            .replace(MAIN_SEPARATOR, "/");
        Ok(root.join(&project_relative)?.cell())
    }

    #[turbo_tasks::function]
    pub async fn pack_path(self: Vc<Self>) -> Result<Vc<FileSystemPath>> {
        let this = self.await?;
        let root = self.project_root().await?;

        let project_relative = if this.pack_path.starts_with(&*this.root_path) {
            this.pack_path.strip_prefix(&*this.root_path).unwrap()
        } else {
            this.pack_path.as_str()
        };
        Ok(root.join(project_relative)?.cell())
    }

    #[turbo_tasks::function]
    pub(super) fn env(&self) -> Vc<Box<dyn ProcessEnv>> {
        *self.process_env
    }

    #[turbo_tasks::function]
    pub(super) fn config(&self) -> Vc<Config> {
        *self.config
    }

    #[turbo_tasks::function]
    pub(super) fn mode(&self) -> Vc<Mode> {
        self.config.mode()
    }

    #[turbo_tasks::function]
    pub(super) fn is_watch_enabled(&self) -> Result<Vc<bool>> {
        Ok(Vc::cell(self.watch.enable))
    }

    #[turbo_tasks::function]
    pub(super) async fn per_entry_module_graph(&self) -> Result<Vc<bool>> {
        Ok(Vc::cell(*self.config.mode().await? == Mode::Development))
    }

    #[turbo_tasks::function]
    pub(super) fn no_mangling(&self) -> Vc<bool> {
        self.config.no_mangling()
    }

    #[turbo_tasks::function]
    pub fn should_create_webpack_stats(&self) -> Vc<bool> {
        self.config.stats()
    }

    #[turbo_tasks::function]
    pub(super) async fn execution_context(self: Vc<Self>) -> Result<Vc<ExecutionContext>> {
        let node_root = self.node_root().owned().await?;
        let mode = self.mode().await?;

        let project_root = self.project_path().owned().await?;
        let node_root_to_root_path = self.node_root_to_root_path().owned().await?;
        let source_maps = if *self.config().source_maps().await? {
            SourceMapsType::Full
        } else {
            SourceMapsType::None
        };

        let build_environment = node_build_environment().to_resolved().await?;
        let execution_chunking_context = Vc::upcast(
            NodeJsChunkingContext::builder(
                project_root,
                node_root.clone(),
                node_root_to_root_path,
                node_root.clone(),
                node_root.clone(),
                node_root.clone(),
                build_environment,
                mode.runtime_type(),
            )
            .source_maps(
                if cfg!(all(target_family = "wasm", target_os = "unknown")) {
                    SourceMapsType::None
                } else {
                    source_maps
                },
            )
            .build(),
        );

        Ok(ExecutionContext::new(
            self.project_path().owned().await?,
            execution_chunking_context,
            self.env(),
        ))
    }

    #[turbo_tasks::function]
    pub(super) async fn client_compile_time_info(&self) -> Result<Vc<CompileTimeInfo>> {
        let mut define_env = (*self.config.define_env().await?).clone();
        define_env.extend((*self.define_env.client().await?).clone());
        Ok(get_client_compile_time_info(
            (*self.config.target().await?).clone(),
            Vc::cell(define_env),
            self.config.mode(),
            self.config.provider_config(),
        ))
    }

    #[turbo_tasks::function]
    pub async fn get_all_endpoints(self: Vc<Self>) -> Result<Vc<Endpoints>> {
        let mut endpoints = vec![];
        let entrypoints = self.entrypoints().await?;
        if let Some(apps) = entrypoints.apps {
            endpoints.extend(apps.await?);
        }
        if let Some(libraries) = entrypoints.libraries {
            endpoints.extend(libraries.await?);
        }
        Ok(Vc::cell(endpoints))
    }

    #[turbo_tasks::function]
    pub async fn get_all_entries(self: Vc<Self>) -> Result<Vc<GraphEntries>> {
        let mut modules = self
            .get_all_endpoints()
            .await?
            .iter()
            .map(async |endpoint| Ok(endpoint.entries().owned().await?))
            .try_flat_join()
            .await?;
        modules.extend(self.client_main_modules().await?.iter().cloned());
        Ok(Vc::cell(modules))
    }

    #[turbo_tasks::function]
    pub async fn get_all_additional_entries(
        self: Vc<Self>,
        graphs: Vc<ModuleGraph>,
    ) -> Result<Vc<GraphEntries>> {
        let modules = self
            .get_all_endpoints()
            .await?
            .iter()
            .map(async |endpoint| Ok(endpoint.additional_entries(graphs).owned().await?))
            .try_flat_join()
            .await?;
        Ok(Vc::cell(modules))
    }

    #[turbo_tasks::function]
    pub async fn module_graph_for_modules(
        self: Vc<Self>,
        evaluatable_assets: Vc<EvaluatableAssets>,
    ) -> Result<Vc<ModuleGraph>> {
        let is_production = self.mode().await?.is_production();
        Ok(if *self.per_entry_module_graph().await? {
            let entries = evaluatable_assets
                .await?
                .iter()
                .copied()
                .map(ResolvedVc::upcast)
                .collect();
            ModuleGraph::from_modules(
                Vc::cell(vec![ChunkGroupEntry::Entry(entries)]),
                is_production,
                is_production,
            )
        } else {
            *self.whole_app_module_graphs().await?.full
        })
    }

    #[turbo_tasks::function]
    pub async fn whole_app_module_graphs(
        self: ResolvedVc<Self>,
    ) -> Result<Vc<BaseAndFullModuleGraph>> {
        async move {
            let module_graphs_op = whole_app_module_graph_operation(self);
            let module_graphs_vc = if self.mode().await?.is_production() {
                module_graphs_op.connect()
            } else {
                // In development mode, we need to to take and drop the issues, otherwise every
                // route will report all issues.
                let vc = module_graphs_op.resolve_strongly_consistent().await?;
                module_graphs_op.drop_issues();
                *vc
            };

            // At this point all modules have been computed and we can get rid of the node.js
            // process pools
            if *self.is_watch_enabled().await? {
                turbopack_node::evaluate::scale_down();
            } else {
                turbopack_node::evaluate::scale_zero();
            }

            Ok(module_graphs_vc)
        }
        .instrument(tracing::trace_span!("module graph for app"))
        .await
    }

    #[turbo_tasks::function]
    pub(super) async fn server_compile_time_info(self: Vc<Self>) -> Result<Vc<CompileTimeInfo>> {
        todo!()
    }

    #[turbo_tasks::function]
    pub(super) async fn edge_compile_time_info(self: Vc<Self>) -> Result<Vc<CompileTimeInfo>> {
        todo!()
    }

    #[turbo_tasks::function]
    pub(super) fn edge_env(&self) -> Vc<EnvMap> {
        todo!()
    }

    #[turbo_tasks::function]
    pub async fn client_chunking_context(self: Vc<Self>) -> Result<Vc<Box<dyn ChunkingContext>>> {
        let output_root_to_root_path = self.node_root_to_root_path().await?;
        Ok(get_client_chunking_context(ClientChunkingContextOptions {
            mode: self.mode(),
            root_path: self.project_path().owned().await?,
            output_root: self.client_root().owned().await?,
            output_root_to_root_path: (*output_root_to_root_path).clone(),
            public_path: self.config().computed_public_path(),
            environment: self.client_compile_time_info().environment(),
            module_id_strategy: self.module_ids(),
            no_mangling: self.no_mangling(),
            config: self.config(),
            export_usage: self.export_usage(),
            unused_references: self.unused_references(),
        }))
    }

    #[turbo_tasks::function]
    pub(super) fn server_chunking_context(
        self: Vc<Self>,
        _client_assets: bool,
    ) -> Vc<NodeJsChunkingContext> {
        todo!()
    }

    #[turbo_tasks::function]
    pub(super) fn edge_chunking_context(
        self: Vc<Self>,
        _client_assets: bool,
    ) -> Vc<Box<dyn ChunkingContext>> {
        todo!()
    }

    #[turbo_tasks::function]
    pub(super) fn runtime_chunking_context(
        self: Vc<Self>,
        client_assets: bool,
        runtime: Runtime,
    ) -> Vc<Box<dyn ChunkingContext>> {
        match runtime {
            Runtime::Edge => self.edge_chunking_context(client_assets),
            Runtime::NodeJs => Vc::upcast(self.server_chunking_context(client_assets)),
        }
    }

    #[turbo_tasks::function]
    pub async fn entrypoints(self: Vc<Self>) -> Result<Vc<Entrypoints>> {
        let library_project = self.library_project().to_resolved().await?.await?;
        let app_project = self.app_project().to_resolved().await?.await?;
        Ok(Entrypoints {
            apps: match *app_project {
                Some(app) => Some(
                    Endpoints(vec![ResolvedVc::upcast(
                        app.get_app_endpoint().to_resolved().await?,
                    )])
                    .resolved_cell(),
                ),
                None => None,
            },
            libraries: match *library_project {
                Some(lib) => {
                    let endpoints: Vec<ResolvedVc<Box<dyn Endpoint>>> = lib
                        .get_library_endpoints()
                        .await?
                        .into_iter()
                        .map(|l| async move {
                            let endpoint: Vc<Box<dyn Endpoint>> = Vc::upcast(**l);
                            endpoint.to_resolved().await
                        })
                        .try_join()
                        .await?;
                    Some(Endpoints(endpoints).resolved_cell())
                }
                None => None,
            },
        }
        .cell())
    }

    #[turbo_tasks::function]
    pub async fn emit_all_output_assets(
        self: Vc<Self>,
        output_assets: OperationVc<OutputAssets>,
    ) -> Result<()> {
        let span = tracing::trace_span!("emitting");
        async move {
            // clean dist director if configured
            if self.config().output().await?.clean.is_some_and(|c| c) {
                let this = self.await?;
                let dist_dir = self.dist_dir().await?;

                // Construct the complete absolute path by combining project_path and dist_dir
                let dist_path = Path::new(&this.project_path).join(&*dist_dir);

                if let Err(e) = clean_directory(&dist_path) {
                    tracing::warn!("Failed to clean dist directory: {}", e);
                }
            }
            let client_root = self.client_root().owned().await?;
            let dist_root = self.dist_root().owned().await?;

            let all_output_assets_op = all_assets_from_entries_operation(output_assets);

            if let Some(map) = self.await?.versioned_content_map {
                // Insert the main output assets
                let _ = map
                    .insert_output_assets(
                        all_output_assets_op,
                        dist_root.clone(),
                        client_root.clone(),
                        dist_root.clone(),
                    )
                    .resolve()
                    .await?;

                Ok(())
            } else {
                let all_output_assets = all_output_assets_op.connect();

                let _ = emit_assets(all_output_assets, dist_root.clone(), client_root, dist_root)
                    .resolve()
                    .await?;

                Ok(())
            }
        }
        .instrument(span)
        .await
    }

    #[turbo_tasks::function]
    async fn hmr_content(self: Vc<Self>, identifier: RcStr) -> Result<Vc<OptionVersionedContent>> {
        if let Some(map) = self.await?.versioned_content_map {
            let content = map.get(self.client_root().await?.join(identifier.as_str())?);
            Ok(content)
        } else {
            bail!("must be in dev mode to hmr")
        }
    }

    #[turbo_tasks::function]
    async fn hmr_version(self: Vc<Self>, identifier: RcStr) -> Result<Vc<Box<dyn Version>>> {
        let content = self.hmr_content(identifier).await?;
        if let Some(content) = &*content {
            Ok(content.version())
        } else {
            Ok(Vc::upcast(NotFoundVersion::new()))
        }
    }

    /// Get the version state for a session. Initialized with the first seen
    /// version in that session.
    #[turbo_tasks::function]
    pub async fn hmr_version_state(
        self: Vc<Self>,
        identifier: RcStr,
        session: TransientInstance<()>,
    ) -> Result<Vc<VersionState>> {
        let version = self.hmr_version(identifier);

        // The session argument is important to avoid caching this function between
        // sessions.
        let _ = session;

        // INVALIDATION: This is intentionally untracked to avoid invalidating this
        // function completely. We want to initialize the VersionState with the
        // first seen version of the session.
        let state = VersionState::new(
            version
                .into_trait_ref()
                .strongly_consistent()
                .untracked()
                .await?,
        )
        .await?;
        Ok(state)
    }

    /// Emits opaque HMR events whenever a change is detected in the chunk group
    /// internally known as `identifier`.
    #[turbo_tasks::function]
    pub async fn hmr_update(
        self: Vc<Self>,
        identifier: RcStr,
        from: Vc<VersionState>,
    ) -> Result<Vc<Update>> {
        let from = from.get();
        let content = self.hmr_content(identifier).await?;
        if let Some(content) = *content {
            Ok(content.update(from))
        } else {
            Ok(Update::Missing.cell())
        }
    }

    /// Gets a list of all HMR identifiers that can be subscribed to. This is
    /// only needed for testing purposes and isn't used in real apps.
    #[turbo_tasks::function]
    pub async fn hmr_identifiers(self: Vc<Self>) -> Result<Vc<Vec<RcStr>>> {
        if let Some(map) = self.await?.versioned_content_map {
            Ok(map.keys_in_path(self.dist_root().owned().await?))
        } else {
            bail!("must be in dev mode to hmr")
        }
    }

    /// Completion when server side changes are detected in output assets
    /// referenced from the roots
    #[turbo_tasks::function]
    pub async fn server_changed(self: Vc<Self>, roots: Vc<OutputAssets>) -> Result<Vc<Completion>> {
        let path = self.node_root().owned().await?;
        Ok(any_output_changed(roots, path, true))
    }

    /// Completion when client side changes are detected in output assets
    /// referenced from the roots
    #[turbo_tasks::function]
    pub async fn client_changed(self: Vc<Self>, roots: Vc<OutputAssets>) -> Result<Vc<Completion>> {
        let path = self.client_root().owned().await?;
        Ok(any_output_changed(roots, path, false))
    }

    #[turbo_tasks::function]
    pub async fn client_main_modules(self: Vc<Self>) -> Result<Vc<GraphEntries>> {
        let modules = vec![];
        // TODO:
        Ok(Vc::cell(modules))
    }

    /// Gets the module id strategy for the project.
    #[turbo_tasks::function]
    pub async fn module_ids(self: Vc<Self>) -> Result<Vc<Box<dyn ModuleIdStrategy>>> {
        let module_id_strategy = match *self.mode().await? {
            Mode::Development => ModuleIdStrategyConfig::Named,
            Mode::Production => self
                .config()
                .module_ids()
                .await?
                .map_or(ModuleIdStrategyConfig::Deterministic, |module_ids| {
                    module_ids
                }),
        };

        match module_id_strategy {
            ModuleIdStrategyConfig::Named => Ok(Vc::upcast(DevModuleIdStrategy::new())),
            ModuleIdStrategyConfig::Deterministic => {
                let module_graphs = self.whole_app_module_graphs().await?;
                Ok(Vc::upcast(get_global_module_id_strategy(
                    *module_graphs.full,
                )))
            }
        }
    }

    /// Compute the used exports and unused imports for each module.
    #[turbo_tasks::function]
    async fn binding_usage_info(self: Vc<Self>) -> Result<Vc<BindingUsageInfo>> {
        let remove_unused_imports = *self.config().remove_unused_imports(self.mode()).await?;

        let module_graphs = self.whole_app_module_graphs().await?;
        Ok(*compute_binding_usage_info(
            module_graphs.full_with_unused_references,
            remove_unused_imports,
        )
        // As a performance optimization, we resolve strongly consistently
        .resolve_strongly_consistent()
        .await?)
    }

    /// Compute the used exports for each module.
    #[turbo_tasks::function]
    pub async fn export_usage(self: Vc<Self>) -> Result<Vc<OptionBindingUsageInfo>> {
        if *self.config().remove_unused_exports(self.mode()).await? {
            Ok(Vc::cell(Some(
                self.binding_usage_info().to_resolved().await?,
            )))
        } else {
            Ok(Vc::cell(None))
        }
    }

    /// Compute the unused references that were removed (inner graph tree shaking).
    #[turbo_tasks::function]
    pub async fn unused_references(self: Vc<Self>) -> Result<Vc<OptionBindingUsageInfo>> {
        if *self.config().remove_unused_imports(self.mode()).await? {
            Ok(Vc::cell(Some(
                self.binding_usage_info().to_resolved().await?,
            )))
        } else {
            Ok(Vc::cell(None))
        }
    }

    #[turbo_tasks::function]
    pub async fn copy_output_assets(self: Vc<Self>) -> Result<Vc<OutputAssets>> {
        let project_path_vc = self.project_path();
        let dist_root_vc = self.dist_root();

        let output_config = self.config().output().await?;
        let copy_config = output_config.copy.as_ref();

        let mut assets = vec![];
        if let Some(patterns) = copy_config {
            let futures: Vec<_> = patterns
                .iter()
                .map(|pattern| async move {
                    let from = pattern.from();
                    let from_path = project_path_vc.await?.join(from.as_str())?;
                    let from_path_vc = from_path.clone().cell();

                    // Check if source is a directory or file
                    let entry_type = from_path.get_type().await?;
                    let mut local_assets = vec![];
                    match *entry_type {
                        FileSystemEntryType::Directory => {
                            let to_base_path = if let Some(to) = pattern.to() {
                                dist_root_vc.await?.join(to)?
                            } else {
                                (*dist_root_vc.await?).clone()
                            };
                            let to_base_path_vc = to_base_path.cell();
                            let dir_assets =
                                copy_directory_recursive_helper(from_path_vc, to_base_path_vc)
                                    .await?;
                            local_assets.extend(dir_assets.iter().copied());
                        }
                        FileSystemEntryType::File => {
                            // For files, if to is not specified, copy to dist root with filename only
                            let to_path = if let Some(to) = pattern.to() {
                                // If to is specified, copy to the specified path relative to dist root
                                dist_root_vc.await?.join(to.as_str())?
                            } else {
                                // Extract just the filename and put it in the dist root
                                let file_name = from_path.file_name();
                                dist_root_vc.await?.join(file_name)?
                            };
                            let source = FileSource::new(from_path);
                            let asset = RawOutput::new(to_path, Vc::upcast(source));
                            local_assets.push(ResolvedVc::upcast(asset.to_resolved().await?));
                        }
                        _ => {}
                    }
                    Ok::<_, anyhow::Error>(local_assets)
                })
                .collect();

            let results = futures::future::try_join_all(futures).await?;
            for sub_assets in results {
                assets.extend(sub_assets);
            }
        }
        Ok(Vc::cell(assets))
    }
}

/// Recursively copy all files from a source directory to a destination directory
/// Preserves the directory structure
#[turbo_tasks::function]
async fn copy_directory_recursive_helper(
    source_dir: Vc<FileSystemPath>,
    dest_dir: Vc<FileSystemPath>,
) -> Result<Vc<OutputAssetVec>> {
    let mut assets = vec![];
    let mut queue = vec![source_dir];
    let source_dir_ref = source_dir.await?;
    let dest_dir_ref = dest_dir.await?;

    while !queue.is_empty() {
        let current_batch = std::mem::take(&mut queue);
        let futures: Vec<_> = current_batch
            .into_iter()
            .map(|path| async move {
                let dir_content = path.await?.read_dir().await?;
                Ok::<_, anyhow::Error>(dir_content)
            })
            .collect();
        let results = futures::future::try_join_all(futures).await?;

        for dir_content in results {
            if let DirectoryContent::Entries(entries) = &*dir_content {
                for entry in entries.values() {
                    match entry {
                        DirectoryEntry::File(file_path) => {
                            let relative_path =
                                source_dir_ref.get_path_to(file_path).ok_or_else(|| {
                                    anyhow::anyhow!("File path is not under source directory")
                                })?;

                            let dest_path = dest_dir_ref.join(relative_path)?;
                            let source = FileSource::new(file_path.clone());
                            let asset = RawOutput::new(dest_path, Vc::upcast(source));
                            assets.push(ResolvedVc::upcast(asset.to_resolved().await?));
                        }
                        DirectoryEntry::Directory(dir_path) => {
                            queue.push(dir_path.clone().cell());
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(Vc::cell(assets))
}

// This is a performance optimization. This function is a root aggregation function that
// aggregates over the whole subgraph.
#[turbo_tasks::function(operation)]
async fn whole_app_module_graph_operation(
    project: ResolvedVc<Project>,
) -> Result<Vc<BaseAndFullModuleGraph>> {
    mark_root();

    let mode = project.mode();
    let mode_ref = mode.await?;
    let should_trace = mode_ref.is_production();
    let should_read_binding_usage = mode_ref.is_production();
    let base_single_module_graph = SingleModuleGraph::new_with_entries(
        project.get_all_entries(),
        should_trace,
        should_read_binding_usage,
    );
    let base_visited_modules = VisitedModules::from_graph(base_single_module_graph);

    let base = ModuleGraph::from_single_graph(base_single_module_graph);

    let remove_unused_imports = *project.config().remove_unused_imports(mode).await?;

    let base = if remove_unused_imports {
        // TODO suboptimal that we do compute_binding_usage_info twice (once for the base graph
        // and later for the full graph)
        base.without_unused_references(
            *compute_binding_usage_info(base.to_resolved().await?, true)
                .resolve_strongly_consistent()
                .await?,
        )
    } else {
        base
    };

    let additional_entries = project.get_all_additional_entries(base);

    let additional_module_graph = SingleModuleGraph::new_with_entries_visited(
        additional_entries,
        base_visited_modules,
        should_trace,
        should_read_binding_usage,
    );

    let full_with_unused_references =
        ModuleGraph::from_graphs(vec![base_single_module_graph, additional_module_graph])
            .to_resolved()
            .await?;

    let full = if remove_unused_imports {
        full_with_unused_references
            .without_unused_references(
                *compute_binding_usage_info(full_with_unused_references, true)
                    .resolve_strongly_consistent()
                    .await?,
            )
            .to_resolved()
            .await?
    } else {
        full_with_unused_references
    };

    Ok(BaseAndFullModuleGraph {
        base: base.to_resolved().await?,
        full_with_unused_references,
        full,
    }
    .cell())
}

#[turbo_tasks::value(shared)]
pub struct BaseAndFullModuleGraph {
    /// The base module graph generated from the entry points.
    pub base: ResolvedVc<ModuleGraph>,
    /// The base graph plus any modules that were generated from additional entries (for which the
    /// base graph is needed).
    pub full_with_unused_references: ResolvedVc<ModuleGraph>,
    /// `full_with_unused_references` but with unused references removed.
    pub full: ResolvedVc<ModuleGraph>,
}

#[turbo_tasks::function]
async fn any_output_changed(
    roots: Vc<OutputAssets>,
    path: FileSystemPath,
    server: bool,
) -> Result<Vc<Completion>> {
    let all_assets = expand_output_assets(
        roots
            .await?
            .into_iter()
            .map(|&a| ExpandOutputAssetsInput::Asset(a)),
        true,
    )
    .await?;
    let completions = all_assets
        .into_iter()
        .map(|m| {
            let path = path.clone();

            async move {
                let asset_path = m.path().await?;
                if !asset_path.path.ends_with(".map")
                    && (!server || !asset_path.path.ends_with(".css"))
                    && asset_path.is_inside_ref(&path)
                {
                    anyhow::Ok(Some(
                        content_changed(*ResolvedVc::upcast(m))
                            .to_resolved()
                            .await?,
                    ))
                } else {
                    Ok(None)
                }
            }
        })
        .try_flat_join()
        .await?;

    Ok(Vc::<Completions>::cell(completions).completed())
}

#[turbo_tasks::function(operation)]
async fn all_assets_from_entries_operation(
    operation: OperationVc<OutputAssets>,
) -> Result<Vc<ExpandedOutputAssets>> {
    let assets = operation.connect();
    Ok(all_assets_from_entries(assets))
}

fn clean_directory(dist_path: &Path) -> Result<()> {
    let canonical_path = fs::canonicalize(dist_path)
        .with_context(|| format!("Failed to canonicalize path: {}", dist_path.display()))?;

    if canonical_path.exists() {
        tracing::info!("Cleaning dist directory: {}", canonical_path.display());

        // Read directory entries
        for entry in fs::read_dir(&canonical_path)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                if let Err(e) = fs::remove_dir_all(&path) {
                    tracing::warn!("Failed to remove directory {}: {}", path.display(), e);
                }
            } else if let Err(e) = fs::remove_file(&path) {
                tracing::warn!("Failed to remove file {}: {}", path.display(), e);
            }
        }

        tracing::info!("Dist directory cleaned successfully");
    } else {
        tracing::debug!("Dist directory does not exist, skipping clean");
    }

    Ok(())
}
