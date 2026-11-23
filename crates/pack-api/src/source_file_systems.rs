use std::{
    collections::HashSet,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use bincode::{Decode, Encode};
use pack_core::config::Config;
use tokio::sync::Notify;
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{OperationValue, OperationVc, ReadRef, ResolvedVc, State, TryJoinIterExt, Vc};
use turbo_tasks_fs::{
    DiskFileSystem, DiskFileSystemMap, DiskWatcherConfig, FileSystem, FileSystemPath,
};
use turbo_unix_path::{normalize_path, unix_to_sys};
use turbopack_core::{
    PROJECT_FILESYSTEM_NAME,
    issue::{Issue, IssueExt, IssueSeverity, IssueStage, StyledString},
};

use crate::project::{strip_root_prefix, strip_root_prefix_for_file_system};

#[turbo_tasks::task_input]
#[derive(Clone, Debug, PartialEq, Eq, Hash, OperationValue, Encode, Decode)]
pub(crate) struct AdditionalRoot {
    name: RcStr,
    path: RcStr,
    cache_directory: Option<RcStr>,
}

#[turbo_tasks::value]
#[derive(Default)]
pub(crate) struct AdditionalRoots {
    pub(crate) roots: Vec<AdditionalRoot>,
    pub(crate) warnings: Vec<RcStr>,
}

#[turbo_tasks::function(fs, session_dependent)]
pub(crate) async fn resolve_additional_roots(
    config: ResolvedVc<Config>,
    project_root: RcStr,
    project_path: RcStr,
) -> Result<Vc<AdditionalRoots>> {
    let cache_directory = config.cache_directory().await?;
    let project_path = Path::new(project_path.as_str());
    let cache_directory =
        canonicalize_cache_path(&project_path.join(unix_to_sys(&cache_directory).as_ref()));
    let config = config.await?;
    let mut roots = AdditionalRoots::default();
    let mut names = HashSet::new();
    for (name, entry) in config.additional_roots() {
        let warning = |reason: &str| {
            RcStr::from(format!(
                "additionalRoots.{name} ({:?}): {reason}",
                entry.path
            ))
        };
        if !valid_root_name(name) {
            roots.warnings.push(warning(
                "use 1–40 ASCII letters, digits, underscores, or hyphens, excluding Windows device names",
            ));
            continue;
        }
        if !names.insert(name.to_ascii_lowercase()) {
            roots
                .warnings
                .push(warning("the name collides with an earlier root"));
            continue;
        }

        let path = project_path.join(unix_to_sys(&entry.path).as_ref());
        let canonical = match canonicalize_directory(&path) {
            Ok(path) => path,
            Err(error) if entry.ignore_if_missing && error.kind() == ErrorKind::NotFound => {
                continue;
            }
            Err(error) => {
                roots.warnings.push(warning(&error.to_string()));
                continue;
            }
        };
        let overlaps = |other: &str| {
            strip_root_prefix(&canonical, other).is_some()
                || strip_root_prefix(other, &canonical).is_some()
        };
        if overlaps(&project_root) {
            roots.warnings.push(warning(&format!(
                "overlaps the project root {project_root:?}"
            )));
            continue;
        }
        if let Some(other) = roots.roots.iter().find(|other| overlaps(&other.path)) {
            roots.warnings.push(warning(&format!(
                "overlaps {} at {:?}",
                other.name, other.path
            )));
            continue;
        }
        if strip_root_prefix(&canonical, &cache_directory).is_some() {
            roots.warnings.push(warning(&format!(
                "lies inside the cache directory {cache_directory:?}"
            )));
            continue;
        }
        roots.roots.push(AdditionalRoot {
            name: RcStr::from(format!("@{name}")),
            cache_directory: strip_root_prefix_for_file_system(&cache_directory, &canonical)
                .and_then(|directory| normalize_path(&directory))
                .filter(|directory| !directory.is_empty())
                .map(RcStr::from),
            path: canonical,
        });
    }
    Ok(roots.cell())
}

fn valid_root_name(name: &str) -> bool {
    if name.is_empty()
        || name.len() > 40
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return false;
    }
    let uppercase = name.to_ascii_uppercase();
    !matches!(uppercase.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !uppercase
            .strip_prefix("COM")
            .or_else(|| uppercase.strip_prefix("LPT"))
            .is_some_and(|suffix| matches!(suffix.as_bytes(), [b'1'..=b'9']))
}

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
fn canonicalize_directory(path: &Path) -> io::Result<RcStr> {
    let canonical = turbo_tasks_fs::canonicalize_to_rcstr(path)?;
    if !std::fs::metadata(canonical.as_str())?.is_dir() {
        return Err(io::Error::new(
            ErrorKind::NotADirectory,
            "the path is not a directory",
        ));
    }
    Ok(canonical)
}

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
fn canonicalize_directory(_path: &Path) -> io::Result<RcStr> {
    Err(io::Error::new(
        ErrorKind::Unsupported,
        "additional roots require a native filesystem",
    ))
}

fn canonicalize_cache_path(path: &Path) -> RcStr {
    // The cache might not exist yet. Resolve its closest existing parent, which can itself be
    // a symlink, and retain the rest of the path for the filesystem's exclusion check.
    let canonical = path.ancestors().find_map(|parent| {
        let canonical = canonicalize_directory(parent).ok()?;
        let remaining = path.strip_prefix(parent).ok()?;
        Some(Path::new(canonical.as_str()).join(remaining))
    });
    RcStr::from(canonical.as_deref().unwrap_or(path).to_string_lossy())
}

#[turbo_tasks::task_input]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Encode, Decode)]
pub(crate) struct SourceFileSystemOptions {
    pub(crate) root: RcStr,
    pub(crate) denied_paths: Vec<RcStr>,
    pub(crate) watcher_config: DiskWatcherConfig,
    pub(crate) watched_ignored: Vec<RcStr>,
    pub(crate) additional_roots: Vec<AdditionalRoot>,
}

#[turbo_tasks::value]
pub(crate) struct SourceFileSystems {
    pub(crate) project: ResolvedVc<DiskFileSystem>,
    additional: Vec<ResolvedVc<DiskFileSystem>>,
}

type FileSystemRoots = Vec<(RcStr, OperationVc<DiskFileSystem>)>;

#[turbo_tasks::value(evict = "never", eq = "manual", cell = "new")]
struct SourceFileSystemMapHolder {
    roots: State<Option<Result<FileSystemRoots, RcStr>>>,
    #[turbo_tasks(debug_ignore, unsafe_ignore)]
    #[bincode(skip)]
    ready: Notify,
}

#[turbo_tasks::function]
fn new_file_system_map_holder(options: SourceFileSystemOptions) -> Vc<SourceFileSystemMapHolder> {
    // Each set of options has its own registry, including when a project updates its roots.
    drop(options);
    SourceFileSystemMapHolder {
        roots: State::new(None),
        ready: Notify::new(),
    }
    .cell()
}

#[turbo_tasks::function]
pub(crate) async fn source_file_systems(
    options: SourceFileSystemOptions,
) -> Result<Vc<SourceFileSystems>> {
    if options.additional_roots.is_empty() {
        return Ok(
            create_source_file_systems(options, DiskFileSystemMap::empty())
                .await?
                .cell(),
        );
    }
    let holder = new_file_system_map_holder(options.clone())
        .to_resolved()
        .await?;
    let paths = std::iter::once(&options.root)
        .chain(options.additional_roots.iter().map(|root| &root.path))
        .cloned()
        .collect::<Vec<_>>();
    let map = disk_file_system_map(holder);
    let result = create_source_file_systems(options, map).await;
    let roots = result
        .as_ref()
        .map(|sources| {
            paths
                .into_iter()
                .zip(
                    std::iter::once(sources.project)
                        .chain(sources.additional.iter().copied())
                        .map(read_file_system),
                )
                .collect()
        })
        .map_err(|error| RcStr::from(format!("{error:#}")));
    let holder = holder.await?;
    holder.roots.update_conditionally(|state| {
        if state.is_some() {
            return false;
        }
        *state = Some(roots);
        true
    });
    holder.ready.notify_waiters();
    Ok(result?.cell())
}

async fn create_source_file_systems(
    options: SourceFileSystemOptions,
    map: OperationVc<DiskFileSystemMap>,
) -> Result<SourceFileSystems> {
    let project = DiskFileSystem::new_with_options_and_watched_ignored(
        PROJECT_FILESYSTEM_NAME,
        Vc::cell(options.root),
        options.denied_paths,
        options.watcher_config,
        options.watched_ignored,
        map,
    )
    .to_resolved()
    .await?;
    let additional = options
        .additional_roots
        .into_iter()
        .map(|root| {
            DiskFileSystem::new_with_options(
                root.name,
                Vc::cell(root.path),
                root.cache_directory.into_iter().collect(),
                options.watcher_config,
                map,
            )
            .to_resolved()
        })
        .try_join()
        .await?;
    Ok(SourceFileSystems {
        project,
        additional,
    })
}

#[turbo_tasks::function(operation, root)]
async fn disk_file_system_map(
    holder: ResolvedVc<SourceFileSystemMapHolder>,
) -> Result<Vc<DiskFileSystemMap>> {
    let holder = holder.await?;
    // TurboTasks may schedule the map before construction returns. Wait for the filesystem
    // identities before reading the map. This state is written once and persisted with the
    // holder, so warm builds do not need to reconstruct it. The read must never register an
    // invalidator since one-shot builds can disable dependency tracking.
    let ready = holder.ready.notified();
    let initialized = holder.roots.get_untracked().is_some();
    if !initialized {
        ready.await;
    }
    let roots = holder
        .roots
        .get_untracked()
        .clone()
        .expect("filesystem map bootstrap was notified before publication")
        .map_err(|error| anyhow!("{error}"))?;
    let roots = roots
        .into_iter()
        .map(async |(path, fs)| {
            Ok((
                PathBuf::from(path.as_str()),
                fs.connect().to_resolved().await?,
            ))
        })
        .try_join()
        .await?;
    Ok(roots.into_iter().collect::<DiskFileSystemMap>().cell())
}

#[turbo_tasks::function(operation, root)]
fn read_file_system(fs: ResolvedVc<DiskFileSystem>) -> Vc<DiskFileSystem> {
    *fs
}

pub(crate) async fn read_source_file_systems(
    operation: OperationVc<SourceFileSystems>,
) -> Result<Vec<ReadRef<DiskFileSystem>>> {
    let sources = operation.read_strongly_consistent().await?;
    std::iter::once(sources.project)
        .chain(sources.additional.iter().copied())
        .map(read_file_system)
        .map(|fs| fs.read_strongly_consistent())
        .try_join()
        .await
}

pub(crate) async fn emit_additional_root_warnings(
    project_fs: ResolvedVc<DiskFileSystem>,
    warnings: &[RcStr],
) -> Result<()> {
    if !warnings.is_empty() {
        let path = project_fs.root().owned().await?;
        for description in warnings {
            AdditionalRootIssue {
                path: path.clone(),
                description: description.clone(),
            }
            .resolved_cell()
            .emit();
        }
    }
    Ok(())
}

#[turbo_tasks::value(shared)]
struct AdditionalRootIssue {
    path: FileSystemPath,
    description: RcStr,
}

#[async_trait]
#[turbo_tasks::value_impl]
impl Issue for AdditionalRootIssue {
    fn severity(&self) -> IssueSeverity {
        IssueSeverity::Warning
    }

    fn stage(&self) -> IssueStage {
        IssueStage::Config
    }

    async fn file_path(&self) -> Result<FileSystemPath> {
        Ok(self.path.clone())
    }

    async fn title(&self) -> Result<StyledString> {
        Ok(StyledString::Text(rcstr!(
            "Invalid Utoopack additional root"
        )))
    }

    async fn description(&self) -> Result<Option<StyledString>> {
        Ok(Some(StyledString::Text(self.description.clone())))
    }
}
