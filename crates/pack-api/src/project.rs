use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use bincode::{Decode, Encode};
#[cfg(any(feature = "process_pool", feature = "worker_pool"))]
use pack_core::config::PluginRuntimeStrategy;
use pack_core::{
    client::context::{
        ClientChunkingContextOptions, get_client_chunking_context, get_client_compile_time_info,
    },
    config::{Config, ModuleIds as ModuleIdStrategyConfig, OptionCompressType, Platform},
    emit_assets,
    library::contexts::{LibraryChunkingContextOptions, get_library_chunking_context},
    mode::Mode,
    server::contexts::{
        ServerChunkingContextOptions, get_server_chunking_context, get_server_compile_time_info,
    },
    util::{Runtime, convert_to_project_relative},
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tracing::{Instrument, field::Empty};
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{
    Completion, Completions, FxIndexMap, NonLocalValue, OperationValue, OperationVc, ReadRef,
    ResolvedVc, State, TransientInstance, TryFlatJoinIterExt, TryJoinIterExt, Vc,
    trace::TraceRawVcs,
};
use turbo_tasks_env::{EnvMap, ProcessEnv};
use turbo_tasks_fs::{
    DirectoryContent, DirectoryEntry, DiskFileSystem, FileContent, FileSystem, FileSystemEntryType,
    FileSystemPath, VirtualFileSystem, invalidation,
};
use turbo_unix_path::{join_path, unix_to_sys};
use turbopack::global_module_ids::get_global_module_id_strategy;
use turbopack_core::{
    chunk::{UnusedReferences, chunk_id_strategy::ModuleIdFallback},
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
        ChunkingContext, EvaluatableAssets, SourceMapsType, chunk_id_strategy::ModuleIdStrategy,
    },
    compile_time_info::CompileTimeInfo,
    issue::{CollectibleIssuesExt, Issue, IssueSeverity, IssueStage, StyledString},
    module::Modules,
    module_graph::{
        GraphEntries, ModuleGraph, SingleModuleGraph, VisitedModules,
        chunk_group_info::{ChunkGroupEntry, EntryHeuristics},
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
#[cfg(feature = "process_pool")]
use turbopack_node::child_process_backend;
use turbopack_node::execution_context::ExecutionContext;
#[cfg(feature = "worker_pool")]
use turbopack_node::worker_threads_backend;
use turbopack_nodejs::NodeJsChunkingContext;

use crate::{
    app::{AppEntrypoint, AppProject, OptionAppProject},
    endpoint::{Endpoint, Endpoints},
    entrypoint::Entrypoints,
    library::{LibraryEntrypoint, LibraryProject, OptionLibraryProject},
    versioned_content_map::VersionedContentMap,
};

#[turbo_tasks::task_input]
#[derive(
    Debug,
    Default,
    Serialize,
    Deserialize,
    Clone,
    PartialEq,
    Eq,
    Hash,
    TraceRawVcs,
    OperationValue,
    Encode,
    Decode,
)]
#[serde(rename_all = "camelCase")]
pub struct WatchOptions {
    /// Whether to watch the filesystem for file changes.
    pub enable: bool,

    /// Enable polling at a certain interval if the native file watching doesn't work (e.g.
    /// docker).
    pub poll_interval: Option<Duration>,

    /// Paths to ignore when watching for file changes.
    /// By default, ignores: node_modules
    #[serde(default = "default_ignored_paths")]
    pub ignored: Vec<RcStr>,
}

pub fn default_ignored_paths() -> Vec<RcStr> {
    vec!["node_modules".into()]
}

fn trim_leading_path_separators(path: &str) -> &str {
    path.trim_start_matches(['/', '\\'])
}

fn normalize_path_for_prefix_match(path: &str) -> String {
    let mut normalized = path.replace('\\', "/");
    while normalized.ends_with('/')
        && normalized.len() > 1
        && !(normalized.len() == 3
            && normalized.as_bytes()[1] == b':'
            && normalized.as_bytes()[2] == b'/')
    {
        normalized.pop();
    }
    normalized
}

fn path_prefix_len(path: &str, root: &str) -> Option<usize> {
    #[cfg(target_family = "windows")]
    {
        let path_cmp = path.to_ascii_lowercase();
        let root_cmp = root.to_ascii_lowercase();
        if !path_cmp.starts_with(&root_cmp) {
            return None;
        }
    }
    #[cfg(not(target_family = "windows"))]
    if !path.starts_with(root) {
        return None;
    }

    if path.len() == root.len()
        || root.ends_with('/')
        || path.as_bytes().get(root.len()) == Some(&b'/')
    {
        Some(root.len())
    } else {
        None
    }
}

fn strip_root_prefix(path: &str, root: &str) -> Option<String> {
    let path_normalized = normalize_path_for_prefix_match(path);
    let root_normalized = normalize_path_for_prefix_match(root);

    if let Some(prefix_len) = path_prefix_len(&path_normalized, &root_normalized) {
        let remainder = &path_normalized[prefix_len..];
        let remainder = remainder.strip_prefix('/').unwrap_or(remainder);
        return Some(unix_to_sys(remainder).into_owned());
    }

    let path_sys = unix_to_sys(path);
    let root_sys = unix_to_sys(root);
    Path::new(path_sys.as_ref())
        .strip_prefix(root_sys.as_ref())
        .ok()
        .map(|relative| trim_leading_path_separators(&relative.to_string_lossy()).to_owned())
}

fn to_file_system_path(path: &str) -> String {
    trim_leading_path_separators(&path.replace('\\', "/")).to_owned()
}

fn strip_root_prefix_for_file_system(path: &str, root: &str) -> Option<String> {
    strip_root_prefix(path, root).map(|relative| to_file_system_path(&relative))
}

fn canonicalize_project_root(root_path: RcStr) -> Result<RcStr> {
    #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
    {
        turbo_tasks_fs::canonicalize_to_rcstr(Path::new(&*root_path))
            .with_context(|| format!("failed to canonicalize project root `{root_path}`"))
    }

    #[cfg(all(target_family = "wasm", target_os = "unknown"))]
    {
        Ok(root_path)
    }
}

fn rebase_path_to_canonical_root(
    path: &str,
    source_root: &str,
    canonical_root: &str,
) -> Option<RcStr> {
    let relative =
        strip_root_prefix(path, source_root).or_else(|| strip_root_prefix(path, canonical_root))?;
    Some(
        Path::new(canonical_root)
            .join(relative)
            .to_string_lossy()
            .into_owned()
            .into(),
    )
}

fn normalize_path_for_canonical_root(
    path: RcStr,
    source_root: &str,
    canonical_root: &RcStr,
) -> RcStr {
    if let Some(path) = rebase_path_to_canonical_root(&path, source_root, canonical_root) {
        return path;
    }

    #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
    if let Ok(canonical_path) = turbo_tasks_fs::canonicalize_to_rcstr(Path::new(&*path))
        && strip_root_prefix(&canonical_path, canonical_root).is_some()
    {
        return canonical_path;
    }

    path
}

fn extend_client_define_env_with_socket_server(
    define_env: &mut FxIndexMap<RcStr, RcStr>,
    socket_server: Option<RcStr>,
) {
    match socket_server {
        Some(socket_server) => {
            define_env.insert(
                "process.env.SOCKET_SERVER".into(),
                serde_json::to_string(socket_server.as_str())
                    .unwrap()
                    .into(),
            );
        }
        None => {
            define_env
                .entry("process.env.SOCKET_SERVER".into())
                .or_insert_with(|| "undefined".into());
        }
    }
}

async fn client_define_env(
    config: Vc<Config>,
    process_env: ResolvedVc<Box<dyn ProcessEnv>>,
) -> Result<Vc<EnvMap>> {
    let mut define_env = (*config.define_env().await?).clone();
    let socket_server = (*process_env.read(rcstr!("SOCKET_SERVER")).await?).clone();

    extend_client_define_env_with_socket_server(&mut define_env, socket_server);

    Ok(Vc::cell(define_env))
}

async fn import_meta_env_base_url(config: ResolvedVc<Config>) -> Result<RcStr> {
    let public_path = config.computed_public_path().owned().await?;

    Ok(match public_path.as_str() {
        "__RUNTIME_PUBLIC_PATH__" | "__AUTO_PUBLIC_PATH__" => rcstr!("/"),
        _ => public_path.clone(),
    })
}

#[derive(
    Debug,
    Deserialize,
    Clone,
    PartialEq,
    Eq,
    TraceRawVcs,
    NonLocalValue,
    OperationValue,
    Encode,
    Decode,
)]
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

    /// Filesystem watcher options.
    pub watch: Option<WatchOptions>,

    /// The build id.
    pub build_id: Option<RcStr>,

    /// Absolute path for `@utoo/pack`.
    pub pack_path: Option<RcStr>,
}

fn normalize_project_options_paths(options: &mut ProjectOptions) -> Result<()> {
    let source_root = options.root_path.clone();
    let canonical_root = canonicalize_project_root(source_root.clone())?;
    options.project_path = normalize_path_for_canonical_root(
        options.project_path.clone(),
        &source_root,
        &canonical_root,
    );
    options.pack_path =
        normalize_path_for_canonical_root(options.pack_path.clone(), &source_root, &canonical_root);
    options.root_path = canonical_root;
    Ok(())
}

fn update_project_option_paths(
    options: &mut ProjectOptions,
    root_path: Option<RcStr>,
    project_path: Option<RcStr>,
    pack_path: Option<RcStr>,
) -> Result<()> {
    let previous_root = options.root_path.clone();
    let source_root = root_path.unwrap_or_else(|| previous_root.clone());
    let canonical_root = canonicalize_project_root(source_root.clone())?;

    if let Some(project_path) = project_path {
        options.project_path =
            normalize_path_for_canonical_root(project_path, &source_root, &canonical_root);
    }
    if let Some(pack_path) = pack_path {
        options.pack_path =
            normalize_path_for_canonical_root(pack_path, &source_root, &canonical_root);
    }
    options.root_path = canonical_root;
    Ok(())
}

#[turbo_tasks::value(transparent)]
#[derive(Default, Debug, Clone, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct DefineEnv(pub Vec<(RcStr, RcStr)>);

#[turbo_tasks::value]
pub struct ProjectContainer {
    name: RcStr,
    options_state: State<Option<ProjectOptions>>,
    versioned_content_map: Option<ResolvedVc<VersionedContentMap>>,
}

#[turbo_tasks::value_impl]
impl ProjectContainer {
    #[turbo_tasks::function(operation, root)]
    pub fn new_operation(name: RcStr, dev: bool) -> Result<Vc<Self>> {
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

#[turbo_tasks::function(operation, root)]
fn project_operation(project: ResolvedVc<ProjectContainer>) -> Vc<Project> {
    project.project()
}

#[turbo_tasks::function(operation, root)]
fn project_fs_operation(project: ResolvedVc<Project>) -> Vc<DiskFileSystem> {
    project.project_fs(project.dist_dir())
}

#[turbo_tasks::function(operation, root)]
fn output_fs_operation(project: ResolvedVc<Project>) -> Vc<DiskFileSystem> {
    project.output_fs()
}

impl ProjectContainer {
    /// Set up filesystems, watchers, and construct the [`Project`] instance inside the container.
    ///
    /// This function is intended to be called inside of [`turbo_tasks::TurboTasks::run`], but not
    /// part of a [`turbo_tasks::function`]. We don't want it to be possibly re-executed.
    ///
    /// This is an associated function instead of a method because we don't currently implement
    /// [`std::ops::Receiver`] on [`OperationVc`].
    // #[tracing::instrument(level = "trace", name = "initialize project", skip_all)]
    pub async fn initialize(this_op: OperationVc<Self>, mut options: ProjectOptions) -> Result<()> {
        let this = this_op.read_strongly_consistent().await?;
        let span = tracing::info_span!(
            "initialize project",
            project_name = %this.name,
            env_diff = Empty
        );

        let span_clone = span.clone();

        async move {
            normalize_project_options_paths(&mut options)?;
            let watch = options.watch.clone();

            this.options_state.set(Some(options));

            #[turbo_tasks::function(operation, root)]
            fn project_from_container_operation(
                container: OperationVc<ProjectContainer>,
            ) -> Vc<Project> {
                container.connect().project()
            }
            let project = project_from_container_operation(this_op)
                .resolve()
                .strongly_consistent()
                .await?;
            let project_fs = project_fs_operation(project)
                .read_strongly_consistent()
                .await?;
            if watch.enable {
                project_fs
                    .start_watching_with_invalidation_reason(watch.poll_interval)
                    .await?;
            } else {
                project_fs.invalidate_with_reason(|path| invalidation::Initialize {
                    // this path is just used for display purposes
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
        .instrument(span_clone)
        .await
    }

    pub async fn update(self: ResolvedVc<Self>, options: PartialProjectOptions) -> Result<()> {
        let span = tracing::info_span!(
            "update project options",
            project_name = %self.await?.name,
            env_diff = Empty
        );
        let span_clone = span.clone();
        async move {
            // HACK: `update` is called from a top-level function. Top-level functions are not
            // allowed to perform eventually consistent reads. Create a stub operation
            // to upgrade the `ResolvedVc` to an `OperationVc`. This is mostly okay
            // because we can assume the `ProjectContainer` was originally resolved with
            // strong consistency, and is rarely updated.
            #[turbo_tasks::function(operation, root)]
            fn project_container_operation_hack(
                container: ResolvedVc<ProjectContainer>,
            ) -> Vc<ProjectContainer> {
                *container
            }
            let this = project_container_operation_hack(self)
                .read_strongly_consistent()
                .await?;
            let PartialProjectOptions {
                root_path,
                project_path,
                config,
                process_env,
                watch,
                build_id,
                pack_path,
            } = options;
            let mut new_options = this
                .options_state
                .get()
                .clone()
                .context("ProjectContainer need to be initialized with initialize()")?;

            update_project_option_paths(&mut new_options, root_path, project_path, pack_path)?;
            if let Some(config) = config {
                new_options.config = config;
            }
            if let Some(process_env) = process_env {
                new_options.process_env = process_env;
            }

            if let Some(watch) = watch {
                new_options.watch = watch;
            }

            if let Some(build_id) = build_id {
                new_options.build_id = build_id;
            }

            // TODO: Handle mode switch, should prevent mode being switched.
            let watch = new_options.watch.clone();

            let project = project_operation(self)
                .resolve()
                .strongly_consistent()
                .await?;
            let prev_project_fs = project_fs_operation(project)
                .read_strongly_consistent()
                .await?;
            let prev_output_fs = output_fs_operation(project)
                .read_strongly_consistent()
                .await?;

            this.options_state.set(Some(new_options));
            let project = project_operation(self)
                .resolve()
                .strongly_consistent()
                .await?;
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
        .instrument(span_clone)
        .await
    }
}

#[turbo_tasks::value_impl]
impl ProjectContainer {
    #[turbo_tasks::function]
    pub async fn project(&self) -> Result<Vc<Project>> {
        let env_map: Vc<EnvMap>;
        let config;
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

    versioned_content_map: Option<ResolvedVc<VersionedContentMap>>,

    build_id: RcStr,

    /// Absolute path for `@utoo/pack`.
    pack_path: RcStr,
}

async fn is_client_hmr_enabled(project: &Project) -> Result<bool> {
    Ok(project.config.mode().await?.is_development()
        && project.watch.enable
        && project.config.dev_server().await?.hot.unwrap_or_default())
}

#[turbo_tasks::value(transparent)]
pub struct ProjectDefineEnv(pub ResolvedVc<EnvMap>);

#[turbo_tasks::value(shared)]
struct ConflictIssue {
    path: FileSystemPath,
    title: ResolvedVc<StyledString>,
    description: ResolvedVc<StyledString>,
    severity: IssueSeverity,
}

#[async_trait]
#[turbo_tasks::value_impl]
impl Issue for ConflictIssue {
    fn stage(&self) -> IssueStage {
        IssueStage::AppStructure
    }

    fn severity(&self) -> IssueSeverity {
        self.severity
    }

    async fn file_path(&self) -> Result<FileSystemPath> {
        Ok(self.path.clone())
    }

    async fn title(&self) -> Result<StyledString> {
        self.title.owned().await
    }

    async fn description(&self) -> Result<Option<StyledString>> {
        Ok(Some(self.description.owned().await?))
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
                e.library.as_ref().map(|l| {
                    anyhow::Ok(LibraryEntrypoint {
                        name: e.name.clone().unwrap_or(
                            PathBuf::from(e.import.as_str())
                                .file_stem()
                                .unwrap()
                                .to_string_lossy()
                                .into(),
                        ),
                        import: convert_to_project_relative(&e.import, &this.project_path)?,
                        runtime_root: l.name.clone(),
                        runtime_export: l.export.clone(),
                    })
                })
            })
            .collect::<Result<Vec<_>>>()?;
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
                                name: e.name.clone().unwrap_or(
                                    PathBuf::from(e.import.as_str())
                                        .file_stem()
                                        .unwrap()
                                        .to_string_lossy()
                                        .into(),
                                ),
                                project: self.to_resolved().await?,
                                import: convert_to_project_relative(&e.import, &this.project_path)?,
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
        let unix_relative_project =
            strip_root_prefix_for_file_system(&self.project_path, &self.root_path)
                .unwrap_or_default();

        let denied_path = denied_path.await?;
        if !denied_path.is_empty() {
            let unix_denied = to_file_system_path(&denied_path);
            if let Some(normalized) = join_path(&unix_relative_project, &unix_denied)
                && !normalized.is_empty()
            {
                denied_paths.push(RcStr::from(normalized));
            }
        }

        if let Some(relative_pack_path) =
            strip_root_prefix_for_file_system(&self.pack_path, &self.root_path)
            && let Some(turbopack_path_normalized) = join_path(&relative_pack_path, ".turbopack")
            && !turbopack_path_normalized.is_empty()
        {
            let turbopack_path_normalized = RcStr::from(turbopack_path_normalized);
            if !denied_paths.contains(&turbopack_path_normalized) {
                denied_paths.push(turbopack_path_normalized);
            }
        }

        let project_turbopack = join_path(&unix_relative_project, ".turbopack")
            .map(RcStr::from)
            .unwrap_or_else(|| rcstr!(".turbopack"));
        if !denied_paths.contains(&project_turbopack) {
            denied_paths.push(project_turbopack);
        }
        // Get watched ignored paths from configuration
        let watched_ignored = self.watch.ignored.clone();

        Ok(DiskFileSystem::new_with_denied_paths_and_watched_ignored(
            PROJECT_FILESYSTEM_NAME,
            Vc::cell(self.root_path.clone()),
            denied_paths,
            watched_ignored,
        ))
    }

    #[turbo_tasks::function]
    pub fn client_fs(self: Vc<Self>) -> Vc<Box<dyn FileSystem>> {
        let virtual_fs = VirtualFileSystem::new_with_name("client-fs".into());
        Vc::upcast(virtual_fs)
    }

    #[turbo_tasks::function]
    pub fn output_fs(&self) -> Vc<DiskFileSystem> {
        DiskFileSystem::new(rcstr!("output"), Vc::cell(self.root_path.clone()))
    }

    #[turbo_tasks::function]
    pub async fn dist_dir_absolute(self: Vc<Self>) -> Result<Vc<RcStr>> {
        let this = self.await?;
        let dist_dir = self.dist_dir().await?;
        Ok(Vc::cell(
            format!(
                "{}{}{}",
                this.root_path,
                std::path::MAIN_SEPARATOR,
                unix_to_sys(
                    &join_path(&this.project_path, dist_dir.as_str())
                        .context("expected project_path to be inside of root_path")?
                )
            )
            .into(),
        ))
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
        let relative_dist_path = to_file_system_path(
            relative_dist_path
                .strip_prefix("./")
                .unwrap_or(&relative_dist_path),
        );

        Ok(Vc::cell(relative_dist_path.into()))
    }

    #[turbo_tasks::function]
    pub async fn node_root(self: Vc<Self>) -> Result<Vc<FileSystemPath>> {
        let this = self.await?;
        let pack_relative = match strip_root_prefix(&this.pack_path, &this.root_path) {
            Some(relative) if !relative.is_empty() => relative,
            _ => ".".to_string(),
        };
        let pack_relative = to_file_system_path(&pack_relative);

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

        let project_relative =
            strip_root_prefix_for_file_system(&this.project_path, &this.root_path).with_context(
                || {
                    format!(
                        "project_path `{}` is not inside root_path `{}`",
                        this.project_path, this.root_path
                    )
                },
            )?;

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

    /// Returns the server output directory name, relative to the project root.
    ///
    /// Reads from `config.server.output.path` if set, otherwise defaults
    /// to `{output.path}/server`.
    #[turbo_tasks::function]
    pub async fn server_dist_dir(self: Vc<Self>) -> Result<Vc<RcStr>> {
        let this = self.await?;
        let server_dir = if let Some(server_path) = this
            .config
            .server()
            .await?
            .output
            .as_ref()
            .and_then(|o| o.path.clone())
        {
            server_path.to_string()
        } else {
            let client_dist = this
                .config
                .output()
                .await?
                .path
                .clone()
                .unwrap_or("dist".into());
            format!("{}/server", client_dist)
        };

        let relative = convert_to_project_relative(&server_dir, &this.project_path)?;
        let relative = to_file_system_path(relative.strip_prefix("./").unwrap_or(&relative));

        Ok(Vc::cell(relative.into()))
    }

    /// Returns the output root for server chunks on the output filesystem.
    #[turbo_tasks::function]
    pub async fn server_dist_root(self: Vc<Self>) -> Result<Vc<FileSystemPath>> {
        let this = self.await?;
        let server_dist_dir = self.server_dist_dir().await?;

        let project_relative =
            strip_root_prefix_for_file_system(&this.project_path, &this.root_path).with_context(
                || {
                    format!(
                        "project_path `{}` is not inside root_path `{}`",
                        this.project_path, this.root_path
                    )
                },
            )?;

        Ok(self
            .output_fs()
            .root()
            .await?
            .join(&project_relative)?
            .join(server_dist_dir.as_str())?
            .cell())
    }

    #[turbo_tasks::function]
    pub async fn node_root_to_root_path(self: Vc<Self>) -> Result<Vc<RcStr>> {
        Ok(Vc::cell(
            self.node_root()
                .await?
                .get_relative_path_to(&*self.output_fs().root().await?)
                .context("Expected node root to be inside of output fs")?,
        ))
    }

    #[turbo_tasks::function]
    pub async fn project_path(self: Vc<Self>) -> Result<Vc<FileSystemPath>> {
        let this = self.await?;
        let root = self.project_root().await?;
        let project_relative =
            strip_root_prefix_for_file_system(&this.project_path, &this.root_path).with_context(
                || {
                    format!(
                        "project_path `{}` is not inside root_path `{}`",
                        this.project_path, this.root_path
                    )
                },
            )?;
        Ok(root.join(&project_relative)?.cell())
    }

    #[turbo_tasks::function]
    pub async fn pack_path(self: Vc<Self>) -> Result<Vc<FileSystemPath>> {
        let this = self.await?;
        let root = self.project_root().await?;

        let project_relative = strip_root_prefix_for_file_system(&this.pack_path, &this.root_path)
            .unwrap_or_else(|| to_file_system_path(&this.pack_path));
        Ok(root.join(&project_relative)?.cell())
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
    pub(super) fn platform(&self) -> Vc<Platform> {
        self.config.platform()
    }

    #[turbo_tasks::function]
    pub(super) fn is_watch_enabled(&self) -> Result<Vc<bool>> {
        Ok(Vc::cell(self.watch.enable))
    }

    #[turbo_tasks::function]
    pub(super) async fn client_hmr_enabled(&self) -> Result<Vc<bool>> {
        Ok(Vc::cell(is_client_hmr_enabled(self).await?))
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
    pub(super) fn compress(&self) -> Vc<OptionCompressType> {
        self.config.compress()
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

        #[cfg(not(any(feature = "process_pool", feature = "worker_pool")))]
        bail!("execution_context requires process_pool or worker_pool feature");

        #[cfg(any(feature = "process_pool", feature = "worker_pool"))]
        {
            let strategy = *self.config().plugin_runtime_strategy().await?;
            let node_backend = match strategy {
                #[cfg(feature = "worker_pool")]
                PluginRuntimeStrategy::WorkerThreads => worker_threads_backend(),
                #[cfg(feature = "process_pool")]
                PluginRuntimeStrategy::ChildProcesses => child_process_backend(),
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
                node_backend,
            ))
        }
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
        let endpoint_entries = self
            .get_all_endpoints()
            .await?
            .iter()
            .map(|endpoint| endpoint.entries().owned())
            .try_join()
            .await?;

        let result = GraphEntries::concatenate(
            endpoint_entries
                .into_iter()
                .chain(std::iter::once(self.client_main_modules().owned().await?)),
        );

        Ok(result.cell())
    }

    #[turbo_tasks::function]
    pub async fn get_all_additional_entries(
        self: Vc<Self>,
        graphs: Vc<ModuleGraph>,
    ) -> Result<Vc<GraphEntries>> {
        let result = GraphEntries::concatenate(
            self.get_all_endpoints()
                .await?
                .iter()
                .map(|endpoint| endpoint.additional_entries(graphs).owned())
                .try_join()
                .await?,
        );
        Ok(result.cell())
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
            ModuleGraph::from_graphs(
                vec![SingleModuleGraph::new_with_entries(
                    GraphEntries::from_chunk_groups(vec![ChunkGroupEntry::Entry {
                        modules: entries,
                        heuristics: EntryHeuristics::default(),
                    }])
                    .resolved_cell(),
                    is_production,
                    is_production,
                )],
                None,
            )
            .connect()
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
                let vc = module_graphs_op.resolve().strongly_consistent().await?;
                module_graphs_op.drop_issues();
                *vc
            };

            // At this point all modules have been computed and we can get rid of the node.js
            // process pools
            let execution_context = self.execution_context().await?;
            let node_backend = execution_context.node_backend.into_trait_ref().await?;
            if *self.is_watch_enabled().await? {
                node_backend.scale_down()?;
            } else {
                node_backend.scale_zero()?;
            }

            Ok(module_graphs_vc)
        }
        .instrument(tracing::trace_span!("module graph for app"))
        .await
    }

    #[turbo_tasks::function]
    pub(super) async fn client_compile_time_info(&self) -> Result<Vc<CompileTimeInfo>> {
        let import_meta_env_base_url = import_meta_env_base_url(self.config).await?;
        Ok(get_client_compile_time_info(
            (*self.config.target().await?).clone(),
            client_define_env(*self.config, self.process_env).await?,
            self.config.mode(),
            self.config.provider_config(),
            import_meta_env_base_url,
            Vc::cell(is_client_hmr_enabled(self).await?),
        ))
    }

    #[turbo_tasks::function]
    pub(super) async fn server_compile_time_info(&self) -> Result<Vc<CompileTimeInfo>> {
        let import_meta_env_base_url = import_meta_env_base_url(self.config).await?;
        Ok(get_server_compile_time_info(
            (*self.config.target().await?).clone(),
            self.config.define_env(),
            self.config.mode(),
            self.config.provider_config(),
            import_meta_env_base_url,
        ))
    }

    /// Returns the appropriate compile-time info for the given platform.
    #[turbo_tasks::function]
    pub(super) async fn compile_time_info_for_platform(&self) -> Result<Vc<CompileTimeInfo>> {
        let target = (*self.config.target().await?).clone();
        let import_meta_env_base_url = import_meta_env_base_url(self.config).await?;
        match &*self.config.platform().await? {
            Platform::Web => Ok(get_client_compile_time_info(
                target,
                client_define_env(*self.config, self.process_env).await?,
                self.config.mode(),
                self.config.provider_config(),
                import_meta_env_base_url,
                Vc::cell(is_client_hmr_enabled(self).await?),
            )),
            Platform::Node => Ok(get_server_compile_time_info(
                target,
                self.config.define_env(),
                self.config.mode(),
                self.config.provider_config(),
                import_meta_env_base_url,
            )),
        }
    }

    #[turbo_tasks::function]
    pub async fn client_chunking_context(self: Vc<Self>) -> Result<Vc<Box<dyn ChunkingContext>>> {
        let mode = self.mode();
        let config = self.config();
        let source_maps = if *config.source_maps().await? {
            SourceMapsType::Full
        } else {
            SourceMapsType::None
        };
        Ok(get_client_chunking_context(ClientChunkingContextOptions {
            mode,
            root_path: self.project_path().owned().await?,
            client_root: self.client_root().owned().await?,
            client_root_to_root_path: rcstr!("/ROOT"),
            public_path: config.computed_public_path(),
            environment: self.client_compile_time_info().environment(),
            module_id_strategy: self.module_ids(),
            export_usage: self.export_usage(),
            unused_references: self.unused_references(),
            minify: config.minify(mode),
            compress: self.compress(),
            source_maps: source_maps.cell(),
            no_mangling: self.no_mangling(),
            scope_hoisting: config.concatenate_modules(mode),
            nested_async_chunking: config.nested_async_chunking(mode),
            debug_ids: Vc::cell(false),
            should_use_absolute_url_references: Vc::cell(false),
            config,
        }))
    }

    #[turbo_tasks::function]
    pub(super) async fn server_chunking_context(
        self: Vc<Self>,
    ) -> Result<Vc<NodeJsChunkingContext>> {
        let mode = self.mode();
        let config = self.config();
        let source_maps = if *config.source_maps().await? {
            SourceMapsType::Full
        } else {
            SourceMapsType::None
        };
        let server_root = self.dist_root().owned().await?;
        Ok(get_server_chunking_context(ServerChunkingContextOptions {
            mode,
            config,
            root_path: server_root.clone(),
            node_root: server_root,
            node_root_to_root_path: rcstr!("/ROOT"),
            environment: self.server_compile_time_info().environment(),
            module_id_strategy: self.module_ids(),
            export_usage: self.export_usage(),
            unused_references: self.unused_references(),
            minify: config.minify(mode),
            compress: self.compress(),
            source_maps: source_maps.cell(),
            no_mangling: self.no_mangling(),
            scope_hoisting: config.concatenate_modules(mode),
            nested_async_chunking: config.nested_async_chunking(mode),
            debug_ids: Vc::cell(false),
        }))
    }

    /// Server chunking context for server functions — uses the library
    /// chunking context which has built-in content hash support (no cycle).
    #[turbo_tasks::function]
    pub(super) async fn server_fn_chunking_context(
        self: Vc<Self>,
    ) -> Result<Vc<Box<dyn ChunkingContext>>> {
        let mode = self.mode();
        let config = self.config();
        let server_root = self.server_dist_root().owned().await?;
        let server_config = config.server().await?;
        let uses_named_server_entries = server_config
            .entry
            .as_ref()
            .is_some_and(|entry| entry.has_named_entries());
        // The legacy scalar server entry historically used the top-level output filename.
        // Apply the server-specific template only for the named multi-entry API.
        let filename_override = uses_named_server_entries
            .then(|| {
                server_config
                    .output
                    .as_ref()
                    .and_then(|output| output.filename.clone())
            })
            .flatten();
        let chunk_filename_override = uses_named_server_entries
            .then(|| {
                server_config
                    .output
                    .as_ref()
                    .and_then(|output| output.chunk_filename.clone())
            })
            .flatten();

        Ok(get_library_chunking_context(
            LibraryChunkingContextOptions {
                name: Vc::cell(Some(rcstr!("index"))),
                preserve_entry_name: true,
                shared_chunks: true,
                filename_override,
                chunk_filename_override,
                mode,
                root_path: server_root.clone(),
                output_root: server_root,
                output_root_to_root_path: rcstr!("/ROOT"),
                environment: self.server_compile_time_info().environment(),
                // Server function modules live in a separate graph, so they
                // can't use the whole-app deterministic ID map. Use named IDs.
                module_id_strategy: ModuleIdStrategy {
                    module_id_map: None,
                    fallback: ModuleIdFallback::Ident,
                }
                .cell(),
                no_mangling: self.no_mangling(),
                compress: self.compress(),
                runtime_root: Vc::cell(None),
                runtime_export: Vc::cell(vec![]),
                config,
                export_usage: Vc::cell(None),
                unused_references: Vc::cell(Default::default()),
                platform: Platform::Node.cell(),
            },
        ))
    }

    /// Build a module graph specifically for server function modules.
    /// Always uses per-entry graph (not whole-app) since server functions
    /// are discovered dynamically and are not part of the app's entry registry.
    #[turbo_tasks::function]
    pub(super) async fn server_fn_module_graph(
        self: Vc<Self>,
        modules: Vc<Modules>,
    ) -> Result<Vc<ModuleGraph>> {
        let is_production = self.mode().await?.is_production();
        let entries = modules.await?.iter().copied().collect();
        Ok(ModuleGraph::from_graphs(
            vec![SingleModuleGraph::new_with_entries(
                GraphEntries::from_chunk_groups(vec![ChunkGroupEntry::Entry {
                    modules: entries,
                    heuristics: EntryHeuristics::default(),
                }])
                .resolved_cell(),
                is_production,
                is_production,
            )],
            None,
        )
        .connect())
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
            Runtime::NodeJs => Vc::upcast(self.server_chunking_context()),
        }
    }

    #[turbo_tasks::function]
    pub async fn entrypoints(self: Vc<Self>) -> Result<Vc<Entrypoints>> {
        let library_project = self.library_project().to_resolved().await?.await?;
        let app_project = self.app_project().to_resolved().await?.await?;
        Ok(Entrypoints {
            apps: match *app_project {
                Some(app) => Some(app.get_app_endpoints().to_resolved().await?),
                None => None,
            },
            libraries: match *library_project {
                Some(lib) => {
                    let endpoints: Vec<ResolvedVc<Box<dyn Endpoint>>> = lib
                        .get_library_endpoints()
                        .await?
                        .into_iter()
                        .map(|l| async move {
                            let endpoint: Vc<Box<dyn Endpoint>> = Vc::upcast(*l);
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
            let client_root = self.client_root().owned().await?;
            let client_output = self.dist_root().owned().await?;
            let output_root = self.output_fs().root().owned().await?;

            let all_output_assets_op = all_assets_from_entries_operation(output_assets);

            if let Some(map) = self.await?.versioned_content_map {
                // Insert the main output assets
                let _ = map
                    .insert_output_assets(
                        all_output_assets_op,
                        output_root.clone(),
                        client_root.clone(),
                        client_output.clone(),
                    )
                    .resolve()
                    .await?;

                Ok(())
            } else {
                let all_output_assets = all_output_assets_op.connect();

                let _ = emit_assets(all_output_assets, output_root, client_root, client_output)
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
        self: ResolvedVc<Self>,
        identifier: RcStr,
        session: TransientInstance<()>,
    ) -> Result<Vc<VersionState>> {
        // The session argument is important to avoid caching this function between
        // sessions.
        let _ = session;

        #[turbo_tasks::function(operation, root)]
        async fn hmr_version_operation(
            this: ResolvedVc<Project>,
            identifier: RcStr,
        ) -> Result<Vc<Box<dyn Version>>> {
            let content = this.hmr_content(identifier).await?;
            if let Some(content) = &*content {
                Ok(content.version())
            } else {
                Ok(Vc::upcast(NotFoundVersion::new()))
            }
        }
        let version_op = hmr_version_operation(self, identifier);

        // INVALIDATION: This is intentionally untracked to avoid invalidating this
        // function completely. We want to initialize the VersionState with the
        // first seen version of the session, not re-create it on every change.
        let state = VersionState::new(
            version_op
                .read_trait_strongly_consistent()
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
            Ok(map.keys_in_path(self.client_root().owned().await?))
        } else {
            bail!("must be in dev mode to hmr")
        }
    }

    /// Completion when server side changes are detected in output assets
    /// referenced from the roots
    #[turbo_tasks::function]
    pub async fn server_changed(self: Vc<Self>, roots: Vc<OutputAssets>) -> Result<Vc<Completion>> {
        // `node_root` contains build-time evaluator assets, not endpoint output assets. Endpoint
        // server outputs are written to `dist_root` and, when configured separately, to
        // `server_dist_root`.
        let paths = vec![
            self.dist_root().owned().await?,
            self.server_dist_root().owned().await?,
        ];
        Ok(any_output_changed(roots, paths, true))
    }

    /// Completion when client side changes are detected in output assets
    /// referenced from the roots
    #[turbo_tasks::function]
    pub async fn client_changed(self: Vc<Self>, roots: Vc<OutputAssets>) -> Result<Vc<Completion>> {
        let path = self.client_root().owned().await?;
        Ok(any_output_changed(roots, vec![path], false))
    }

    #[turbo_tasks::function]
    pub async fn client_main_modules(self: Vc<Self>) -> Result<Vc<GraphEntries>> {
        // TODO:
        Ok(GraphEntries::empty())
    }

    /// Gets the module id strategy for the project.
    #[turbo_tasks::function]
    pub async fn module_ids(self: Vc<Self>) -> Result<Vc<ModuleIdStrategy>> {
        let module_id_strategy = match *self.mode().await? {
            Mode::Development => ModuleIdStrategyConfig::Named,
            Mode::Production => self
                .config()
                .module_ids()
                .await?
                .unwrap_or(ModuleIdStrategyConfig::Deterministic),
        };

        match module_id_strategy {
            ModuleIdStrategyConfig::Named => Ok(ModuleIdStrategy {
                module_id_map: None,
                fallback: ModuleIdFallback::Ident,
            }
            .cell()),
            ModuleIdStrategyConfig::Deterministic => {
                let module_graphs = self.whole_app_module_graphs().await?;
                Ok(get_global_module_id_strategy(*module_graphs.full))
            }
        }
    }

    #[turbo_tasks::function]
    async fn binding_usage_info(self: Vc<Self>) -> Result<Vc<BindingUsageInfo>> {
        let module_graphs = self.whole_app_module_graphs().await?;
        Ok(module_graphs
            .binding_usage_info
            .context("No binding usage info")?
            .connect())
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
    pub async fn unused_references(self: Vc<Self>) -> Result<Vc<UnusedReferences>> {
        if *self.config().remove_unused_imports(self.mode()).await? {
            Ok(self.binding_usage_info().unused_references())
        } else {
            Ok(Vc::cell(Default::default()))
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
#[turbo_tasks::function(operation, root)]
async fn whole_app_module_graph_operation(
    project: ResolvedVc<Project>,
) -> Result<Vc<BaseAndFullModuleGraph>> {
    let mode = project.mode();
    let mode_ref = mode.await?;
    let should_trace = mode_ref.is_production();
    let should_read_binding_usage = mode_ref.is_production();
    let base_single_module_graph = SingleModuleGraph::new_with_entries(
        project.get_all_entries().to_resolved().await?,
        should_trace,
        should_read_binding_usage,
    );
    let base_visited_modules = VisitedModules::from_graph(base_single_module_graph);

    let base = ModuleGraph::from_graphs(vec![base_single_module_graph], None);

    let remove_unused_imports = *project.config().remove_unused_imports(mode).await?;

    let base = if remove_unused_imports {
        // TODO suboptimal that we do compute_binding_usage_info twice (once for the base graph
        // and later for the full graph)
        let binding_usage_info = compute_binding_usage_info(base, true);
        ModuleGraph::from_graphs(vec![base_single_module_graph], Some(binding_usage_info))
    } else {
        base
    };

    let additional_entries = project
        .get_all_additional_entries(base.connect())
        .to_resolved()
        .await?;

    let additional_module_graph = SingleModuleGraph::new_with_entries_visited(
        additional_entries,
        base_visited_modules,
        should_trace,
        should_read_binding_usage,
    );

    let graphs = vec![base_single_module_graph, additional_module_graph];

    let (full, binding_usage_info) = if remove_unused_imports {
        let full_with_unused_references = ModuleGraph::from_graphs(graphs.clone(), None);
        let binding_usage_info = compute_binding_usage_info(full_with_unused_references, true);
        (
            ModuleGraph::from_graphs(graphs, Some(binding_usage_info)),
            Some(binding_usage_info),
        )
    } else {
        (ModuleGraph::from_graphs(graphs, None), None)
    };

    Ok(BaseAndFullModuleGraph {
        base: base.connect().to_resolved().await?,
        full: full.connect().to_resolved().await?,
        binding_usage_info,
    }
    .cell())
}

#[turbo_tasks::value(shared)]
pub struct BaseAndFullModuleGraph {
    /// The base module graph generated from the entry points.
    pub base: ResolvedVc<ModuleGraph>,
    /// The base graph plus any modules that were generated from additional entries (for which the
    /// base graph is needed).
    pub binding_usage_info: Option<OperationVc<BindingUsageInfo>>,
    /// `full_with_unused_references` but with unused references removed.
    pub full: ResolvedVc<ModuleGraph>,
}

#[turbo_tasks::function]
async fn any_output_changed(
    roots: Vc<OutputAssets>,
    paths: Vec<FileSystemPath>,
    server: bool,
) -> Result<Vc<Completion>> {
    let all_assets = expand_output_assets(
        roots.await?.into_iter().map(ExpandOutputAssetsInput::Asset),
        true,
    )
    .await?;
    let completions = all_assets
        .into_iter()
        .map(|m| {
            let paths = paths.clone();

            async move {
                let asset_path = m.path().await?;
                if !asset_path.path.ends_with(".map")
                    && (!server || !asset_path.path.ends_with(".css"))
                    && paths.iter().any(|path| asset_path.is_inside_ref(path))
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

#[turbo_tasks::function(operation, root)]
async fn all_assets_from_entries_operation(
    operation: OperationVc<OutputAssets>,
) -> Result<Vc<ExpandedOutputAssets>> {
    let assets = operation.connect();
    Ok(all_assets_from_entries(assets))
}

#[cfg(test)]
mod tests {
    use super::{
        ProjectOptions, WatchOptions, normalize_project_options_paths, strip_root_prefix,
        strip_root_prefix_for_file_system, to_file_system_path, update_project_option_paths,
    };
    use turbo_unix_path::unix_to_sys;

    #[test]
    fn strip_root_prefix_handles_separator_differences() {
        assert_eq!(
            strip_root_prefix("C:/repo/app", r"C:\repo").as_deref(),
            Some(unix_to_sys("app").as_ref())
        );
    }

    #[test]
    fn strip_root_prefix_rejects_non_boundary_prefixes() {
        assert_eq!(strip_root_prefix("/repo-app", "/repo"), None);
    }

    #[test]
    fn strip_root_prefix_supports_trailing_root_separator() {
        assert_eq!(
            strip_root_prefix("/repo/app", "/repo/").as_deref(),
            Some("app")
        );
    }

    #[test]
    fn file_system_paths_use_unix_separators() {
        assert_eq!(
            to_file_system_path(r"\examples\with-use-model\.umi\plugin-model\index.tsx"),
            "examples/with-use-model/.umi/plugin-model/index.tsx"
        );
    }

    #[test]
    fn strip_root_prefix_for_file_system_normalizes_windows_relative_path() {
        assert_eq!(
            strip_root_prefix_for_file_system(
                r"D:\a\umi\umi\examples\with-use-model",
                r"D:\a\umi\umi"
            )
            .as_deref(),
            Some("examples/with-use-model")
        );
    }

    #[cfg(target_family = "windows")]
    #[test]
    fn strip_root_prefix_is_case_insensitive_on_windows() {
        assert_eq!(
            strip_root_prefix("C:/Repo/App", "c:/repo").as_deref(),
            Some("App")
        );
    }

    #[cfg(unix)]
    fn test_project_options(root: &std::path::Path) -> ProjectOptions {
        ProjectOptions {
            root_path: root.to_string_lossy().into_owned().into(),
            project_path: root.join("project").to_string_lossy().into_owned().into(),
            config: "{}".into(),
            process_env: Vec::new(),
            watch: WatchOptions::default(),
            dev: false,
            build_id: "test".into(),
            pack_path: root.join("pack").to_string_lossy().into_owned().into(),
        }
    }

    #[cfg(unix)]
    fn create_test_root(name: &str) -> std::path::PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "utoo-pack-api-{name}-{}-{unique}",
            std::process::id()
        ))
    }

    #[cfg(unix)]
    #[test]
    fn project_paths_follow_canonical_symlink_root() {
        use std::{fs, os::unix::fs::symlink};

        let base = create_test_root("canonical-paths");
        let real_root = base.join("real");
        let alias_root = base.join("alias");
        fs::create_dir_all(real_root.join("project")).unwrap();
        fs::create_dir_all(real_root.join("pack")).unwrap();
        symlink(&real_root, &alias_root).unwrap();
        let canonical_real_root = fs::canonicalize(&real_root).unwrap();

        let mut options = test_project_options(&alias_root);
        normalize_project_options_paths(&mut options).unwrap();

        assert_eq!(
            options.root_path.as_str(),
            canonical_real_root.to_string_lossy()
        );
        assert_eq!(
            options.project_path.as_str(),
            canonical_real_root.join("project").to_string_lossy()
        );
        assert_eq!(
            options.pack_path.as_str(),
            canonical_real_root.join("pack").to_string_lossy()
        );

        update_project_option_paths(
            &mut options,
            None,
            Some(
                alias_root
                    .join("project")
                    .to_string_lossy()
                    .into_owned()
                    .into(),
            ),
            Some(
                alias_root
                    .join("pack")
                    .to_string_lossy()
                    .into_owned()
                    .into(),
            ),
        )
        .unwrap();
        assert_eq!(
            options.project_path.as_str(),
            canonical_real_root.join("project").to_string_lossy()
        );
        assert_eq!(
            options.pack_path.as_str(),
            canonical_real_root.join("pack").to_string_lossy()
        );

        fs::remove_dir_all(base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn root_only_update_preserves_absolute_project_and_pack_paths() {
        use std::{fs, os::unix::fs::symlink};

        let base = create_test_root("root-update");
        let new_root = base.join("repo");
        let old_root = new_root.join("app");
        let new_root_alias = base.join("new-alias");
        fs::create_dir_all(old_root.join("project")).unwrap();
        fs::create_dir_all(old_root.join("pack")).unwrap();
        symlink(&new_root, &new_root_alias).unwrap();
        let canonical_new_root = fs::canonicalize(&new_root).unwrap();
        let canonical_old_root = fs::canonicalize(&old_root).unwrap();

        let mut options = test_project_options(&old_root);
        normalize_project_options_paths(&mut options).unwrap();
        update_project_option_paths(
            &mut options,
            Some(new_root_alias.to_string_lossy().into_owned().into()),
            None,
            None,
        )
        .unwrap();

        assert_eq!(
            options.root_path.as_str(),
            canonical_new_root.to_string_lossy()
        );
        assert_eq!(
            options.project_path.as_str(),
            canonical_old_root.join("project").to_string_lossy()
        );
        assert_eq!(
            options.pack_path.as_str(),
            canonical_old_root.join("pack").to_string_lossy()
        );

        fs::remove_dir_all(base).unwrap();
    }
}
