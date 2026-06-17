use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use bytes::Bytes;
use futures::stream::{FuturesUnordered, StreamExt};
use tokio::sync::{mpsc, oneshot};
use utoo_ruborist::progress::{BuildEvent, EventReceiver, PackageTarballInfo};

use crate::service::auth;
use crate::util::cloner::{PackageClone, clone_package_sync};
use crate::util::downloader::download_bytes;
use crate::util::package_cache::{
    CachePlan, ExtractOutcome, extract_non_registry_to_target, extract_to_cache,
    registry_cache_lookup, resolve_cache_plan,
};
use crate::util::user_config::{get_manifests_concurrency_limit_sync, is_registry_tarball};

/// Build event receiver that forwards download prefetch work to the scheduler.
pub(crate) struct InstallEventReceiver<R: EventReceiver> {
    scheduler: InstallScheduler,
    inner: R,
}

impl<R: EventReceiver> InstallEventReceiver<R> {
    pub(crate) fn new(inner: R, scheduler: InstallScheduler) -> Self {
        Self { scheduler, inner }
    }
}

impl<R: EventReceiver> EventReceiver for InstallEventReceiver<R> {
    fn on_event(&self, event: BuildEvent<'_>) {
        self.inner.on_event(event);

        // Events only warm the download cache (speculative, idempotent). The
        // resolver is omit-agnostic and its events carry no dev/optional flags,
        // so cloning into node_modules — the authoritative, omit/platform/link
        // -filtered action — is left entirely to `ensure_clone` in
        // `install_packages`. Eager event-driven cloning here would place
        // omitted (e.g. --omit=dev) packages on disk that the install pass
        // never asked for.
        if let BuildEvent::PackageResolved(info) = event {
            self.scheduler.prefetch_tarball(&info);
        }
    }
}

#[derive(Clone, Debug)]
struct PackageRef {
    name: String,
    version: String,
    tarball_url: String,
}

impl PackageRef {
    fn key(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }
}

#[derive(Clone, Debug)]
struct CloneSpec {
    package: PackageRef,
    target: PathBuf,
    parent: Option<PathBuf>,
}

struct ReadyClone {
    spec: CloneSpec,
    cache_path: PathBuf,
}

struct DownloadedPackage {
    package: PackageRef,
    bytes: Bytes,
}

type CloneResponder = oneshot::Sender<Result<(), String>>;

enum Command {
    PrefetchDownload(PackageRef),
    EnsureClone(CloneSpec, CloneResponder),
    Shutdown,
}

/// Completion report for one pipeline stage (cache-resolve / download /
/// extract / clone), fed back into the scheduler loop via `handle_report`.
/// Async stages arrive on `async_ops`; the rayon clone stage arrives on
/// `clone_done_rx`.
enum StageReport {
    CacheResolved {
        spec: CloneSpec,
        result: Result<CachePlan, String>,
    },
    Download {
        package: PackageRef,
        result: Result<DownloadOutcome, String>,
    },
    Extract {
        key: String,
        result: Result<ExtractOutcome, String>,
    },
    /// A non-registry tarball was fetched/read and extracted directly into its
    /// `node_modules` target (no global cache). The target is fully materialized,
    /// so this completes the clone for that target outright.
    DirectExtract {
        target: PathBuf,
        result: Result<(), String>,
    },
    Clone {
        target: PathBuf,
        result: Result<bool, String>,
    },
}

enum DownloadOutcome {
    Cached(PathBuf),
    Bytes(Bytes),
}

/// pnpm-style install outcome counts, owned by the scheduler instead of global
/// atomic counters in the util layer.
#[derive(Default, Clone, Copy)]
pub(crate) struct InstallCounts {
    pub downloaded: usize,
    pub reused: usize,
    pub cloned: usize,
}

#[derive(Clone)]
pub(crate) struct InstallScheduler {
    tx: mpsc::UnboundedSender<Command>,
}

pub(crate) struct InstallSchedulerHandle {
    scheduler: InstallScheduler,
    handle: tokio::task::JoinHandle<InstallCounts>,
}

impl InstallSchedulerHandle {
    pub(crate) fn start() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let handle = tokio::spawn(async move { SchedulerState::new(rx).run().await });
        Self {
            scheduler: InstallScheduler { tx },
            handle,
        }
    }

    pub(crate) fn scheduler(&self) -> InstallScheduler {
        self.scheduler.clone()
    }

    /// Signal shutdown, wait for in-flight work to drain, and return the
    /// scheduler's own install counts.
    pub(crate) async fn shutdown(self) -> InstallCounts {
        let _ = self.scheduler.tx.send(Command::Shutdown);
        match self.handle.await {
            Ok(counts) => counts,
            Err(e) => {
                tracing::warn!("Install scheduler task failed: {e}");
                InstallCounts::default()
            }
        }
    }
}

impl InstallScheduler {
    pub(crate) fn prefetch_download(&self, name: String, version: String, tarball_url: String) {
        // Only registry-host tarballs use the global download cache; git and
        // non-registry (file:/untrusted-https) tarballs are not prefetched here
        // (git is cloned in BFS; non-registry tarballs are materialized straight
        // into node_modules at clone time).
        if !is_registry_tarball(&tarball_url) {
            return;
        }
        let _ = self.tx.send(Command::PrefetchDownload(PackageRef {
            name,
            version,
            tarball_url,
        }));
    }

    /// Single prefetch gate shared by the resolver event path
    /// ([`InstallEventReceiver`]) and the lockfile seed in `install()`: seed a
    /// package's tarball download only when it targets this platform and has a
    /// registry tarball. Centralizing the platform filter here means neither
    /// caller can forget it — an earlier divergence (one path checked
    /// platform, the other didn't) over-downloaded incompatible binaries.
    pub(crate) fn prefetch_tarball(&self, info: &PackageTarballInfo<'_>) {
        if info.is_platform_compatible()
            && let Some(url) = info.tarball_url
        {
            self.prefetch_download(
                info.name.to_string(),
                info.version.to_string(),
                url.to_string(),
            );
        }
    }

    pub(crate) async fn ensure_clone(
        &self,
        name: String,
        version: String,
        tarball_url: String,
        target: PathBuf,
    ) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Command::EnsureClone(
                CloneSpec {
                    package: PackageRef {
                        name,
                        version,
                        tarball_url,
                    },
                    target,
                    parent: None,
                },
                tx,
            ))
            .map_err(|_| anyhow!("install scheduler stopped"))?;
        rx.await
            .context("install scheduler stopped before clone completed")?
            .map_err(anyhow::Error::msg)
    }
}

/// Shared admission control for a pipeline stage (download / extract / clone).
///
/// Drains admissible items from `queue` into the in-flight set (`active`) up to
/// `limit`, skipping any key already completed or in flight, and returns the
/// admitted `(item, key)` pairs for the caller to spawn. This keeps the
/// bounded single-flight scheduling logic in one place; each stage only
/// supplies its key function, its "already done" predicate, and how it spawns
/// the work (async task vs rayon), which genuinely differ per stage.
fn admit_stage<I, K>(
    queue: &mut VecDeque<I>,
    active: &mut HashSet<K>,
    limit: usize,
    key_of: impl Fn(&I) -> K,
    mut already_done: impl FnMut(&K) -> bool,
) -> Vec<(I, K)>
where
    K: Eq + std::hash::Hash + Clone,
{
    let mut admitted = Vec::new();
    while active.len() < limit {
        let Some(item) = queue.pop_front() else {
            break;
        };
        let key = key_of(&item);
        if already_done(&key) || !active.insert(key.clone()) {
            continue;
        }
        admitted.push((item, key));
    }
    admitted
}

struct SchedulerState {
    rx: mpsc::UnboundedReceiver<Command>,
    shutdown: bool,
    download_limit: usize,
    extract_limit: usize,
    clone_limit: usize,
    download_done: HashMap<String, PathBuf>,
    download_active: HashSet<String>,
    fetch_waiters: HashMap<String, Vec<CloneSpec>>,
    download_queue: VecDeque<PackageRef>,
    extract_active: HashSet<String>,
    extract_queue: VecDeque<DownloadedPackage>,
    direct_extract_limit: usize,
    direct_extract_active: HashSet<PathBuf>,
    direct_extract_queue: VecDeque<CloneSpec>,
    clone_done: HashSet<PathBuf>,
    clone_active: HashSet<PathBuf>,
    clone_waiters: HashMap<PathBuf, Vec<CloneResponder>>,
    blocked_by_parent: HashMap<PathBuf, Vec<CloneSpec>>,
    clone_queue: VecDeque<ReadyClone>,
    clone_done_tx: mpsc::UnboundedSender<StageReport>,
    clone_done_rx: mpsc::UnboundedReceiver<StageReport>,
    async_ops: FuturesUnordered<tokio::task::JoinHandle<StageReport>>,
    counts: InstallCounts,
}

impl SchedulerState {
    fn new(rx: mpsc::UnboundedReceiver<Command>) -> Self {
        let (clone_done_tx, clone_done_rx) = mpsc::unbounded_channel();
        Self {
            rx,
            shutdown: false,
            download_limit: get_manifests_concurrency_limit_sync().max(1),
            extract_limit: extract_concurrency_limit(),
            clone_limit: clone_concurrency_limit(),
            download_done: HashMap::new(),
            download_active: HashSet::new(),
            fetch_waiters: HashMap::new(),
            download_queue: VecDeque::new(),
            extract_active: HashSet::new(),
            extract_queue: VecDeque::new(),
            direct_extract_limit: extract_concurrency_limit(),
            direct_extract_active: HashSet::new(),
            direct_extract_queue: VecDeque::new(),
            clone_done: HashSet::new(),
            clone_active: HashSet::new(),
            clone_waiters: HashMap::new(),
            blocked_by_parent: HashMap::new(),
            clone_queue: VecDeque::new(),
            clone_done_tx,
            clone_done_rx,
            async_ops: FuturesUnordered::new(),
            counts: InstallCounts::default(),
        }
    }

    async fn run(mut self) -> InstallCounts {
        loop {
            self.pump_downloads();
            self.pump_extracts();
            self.pump_direct_extracts();
            self.pump_clones();

            if self.shutdown && self.is_idle() {
                break;
            }

            tokio::select! {
                command = self.rx.recv(), if !self.shutdown => {
                    match command {
                        Some(Command::PrefetchDownload(package)) => {
                            self.ensure_download(package, None);
                        }
                        Some(Command::EnsureClone(spec, responder)) => {
                            self.queue_clone(spec, Some(responder));
                        }
                        Some(Command::Shutdown) | None => {
                            self.shutdown = true;
                        }
                    }
                }
                done = self.async_ops.next(), if !self.async_ops.is_empty() => {
                    match done {
                        Some(Ok(done)) => self.handle_report(done),
                        // A JoinError means a stage task panicked: it produced no
                        // StageReport, so its active-set slot and the responder
                        // waiting on it can't be cleaned up by key. Rather than
                        // leak the slot and hang the awaiting `ensure_clone`
                        // forever, fail loudly — drain every pending responder
                        // with an error and stop, so the install aborts cleanly.
                        Some(Err(e)) => {
                            tracing::error!("Install scheduler worker panicked: {e}");
                            self.fail_pending("install scheduler worker panicked");
                            break;
                        }
                        None => {}
                    }
                }
                done = self.clone_done_rx.recv() => {
                    if let Some(done) = done {
                        self.handle_report(done);
                    }
                }
            }
        }

        self.fail_pending("install scheduler stopped before work completed");
        self.counts
    }

    fn is_idle(&self) -> bool {
        self.download_queue.is_empty()
            && self.extract_queue.is_empty()
            && self.direct_extract_queue.is_empty()
            && self.clone_queue.is_empty()
            && self.download_active.is_empty()
            && self.extract_active.is_empty()
            && self.direct_extract_active.is_empty()
            && self.clone_active.is_empty()
            && self.async_ops.is_empty()
    }

    fn queue_clone(&mut self, spec: CloneSpec, responder: Option<CloneResponder>) {
        let target = spec.target.clone();
        if self.clone_done.contains(&target) {
            if let Some(responder) = responder {
                let _ = responder.send(Ok(()));
            }
            return;
        }

        if let Some(waiters) = self.clone_waiters.get_mut(&target) {
            if let Some(responder) = responder {
                waiters.push(responder);
            }
            return;
        }

        self.clone_waiters
            .insert(target.clone(), responder.into_iter().collect());

        if let Some(parent) = &spec.parent
            && self.clone_waiters.contains_key(parent)
            && !self.clone_done.contains(parent)
        {
            self.blocked_by_parent
                .entry(parent.clone())
                .or_default()
                .push(spec);
            return;
        }

        self.resolve_cache_for_clone(spec);
    }

    fn resolve_cache_for_clone(&mut self, spec: CloneSpec) {
        let task_spec = spec.clone();
        self.async_ops.push(tokio::spawn(async move {
            let result = resolve_cache_plan(
                &task_spec.package.name,
                &task_spec.package.version,
                &task_spec.package.tarball_url,
            )
            .await
            .map_err(|e| format!("{e:#}"));
            StageReport::CacheResolved { spec, result }
        }));
    }

    fn ensure_download(&mut self, package: PackageRef, waiter: Option<CloneSpec>) {
        let key = package.key();
        if let Some(cache_path) = self.download_done.get(&key).cloned() {
            if let Some(spec) = waiter {
                self.clone_queue.push_back(ReadyClone { spec, cache_path });
            }
            return;
        }

        if let Some(waiters) = self.fetch_waiters.get_mut(&key) {
            if let Some(spec) = waiter {
                waiters.push(spec);
            }
            return;
        }

        self.fetch_waiters.insert(key, waiter.into_iter().collect());
        self.download_queue.push_back(package);
    }

    fn pump_downloads(&mut self) {
        // Bound downloaded tarballs waiting for extraction so network prefetch
        // cannot outrun CPU/disk extraction and pile up Bytes. The extract
        // backlog (queued + in-flight) is unaffected by this pump, so gating
        // once up front matches the original per-iteration check.
        if self.extract_queue.len() + self.extract_active.len() >= self.download_limit {
            return;
        }
        let done = &self.download_done;
        let admitted = admit_stage(
            &mut self.download_queue,
            &mut self.download_active,
            self.download_limit,
            PackageRef::key,
            |key| done.contains_key(key),
        );
        for (package, _key) in admitted {
            self.async_ops.push(tokio::spawn(async move {
                let result = match registry_cache_lookup(&package.name, &package.version).await {
                    Ok(Some(cache_path)) => Ok(DownloadOutcome::Cached(cache_path)),
                    Ok(None) => {
                        let token = auth::token_for_url(&package.tarball_url).await;
                        download_bytes(&package.tarball_url, token.as_deref())
                            .await
                            .map(DownloadOutcome::Bytes)
                            .map_err(|e| format!("{e:#}"))
                    }
                    Err(e) => Err(format!("{e:#}")),
                };
                StageReport::Download { package, result }
            }));
        }
    }

    fn pump_extracts(&mut self) {
        let done = &self.download_done;
        let admitted = admit_stage(
            &mut self.extract_queue,
            &mut self.extract_active,
            self.extract_limit,
            |downloaded| downloaded.package.key(),
            |key| done.contains_key(key),
        );
        for (downloaded, key) in admitted {
            self.async_ops.push(tokio::spawn(async move {
                let _gauge = crate::util::install_progress::GaugeGuard::extracting();
                let result = extract_to_cache(
                    &downloaded.package.name,
                    &downloaded.package.version,
                    downloaded.bytes,
                )
                .await
                .map_err(|e| format!("{e:#}"));
                StageReport::Extract { key, result }
            }));
        }
    }

    fn pump_clones(&mut self) {
        let done = &self.clone_done;
        let admitted = admit_stage(
            &mut self.clone_queue,
            &mut self.clone_active,
            self.clone_limit,
            |job| job.spec.target.clone(),
            |key| done.contains(key),
        );
        for (job, target) in admitted {
            let clone_done_tx = self.clone_done_tx.clone();
            rayon::spawn(move || {
                let _gauge = crate::util::install_progress::GaugeGuard::linking();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    clone_package_sync(&PackageClone {
                        name: &job.spec.package.name,
                        version: &job.spec.package.version,
                        tarball_url: &job.spec.package.tarball_url,
                        cache: &job.cache_path,
                        target: &job.spec.target,
                    })
                    .map_err(|e| format!("{e:#}"))
                }))
                .unwrap_or_else(|_| Err("install clone worker panicked".to_string()));
                let _ = clone_done_tx.send(StageReport::Clone { target, result });
            });
        }
    }

    /// Materialize non-registry tarballs ([`CachePlan::DirectExtract`]) straight
    /// into their `node_modules` target. Bounded the same way as registry
    /// extraction; each completed target is reported via `DirectExtract` and
    /// finalizes that clone outright (no global cache, no cloner step).
    fn pump_direct_extracts(&mut self) {
        let done = &self.clone_done;
        let admitted = admit_stage(
            &mut self.direct_extract_queue,
            &mut self.direct_extract_active,
            self.direct_extract_limit,
            |spec| spec.target.clone(),
            |key| done.contains(key),
        );
        for (spec, target) in admitted {
            self.async_ops.push(tokio::spawn(async move {
                let result =
                    extract_non_registry_to_target(&spec.package.tarball_url, &spec.target)
                        .await
                        .map_err(|e| format!("{e:#}"));
                StageReport::DirectExtract { target, result }
            }));
        }
    }

    fn handle_report(&mut self, done: StageReport) {
        match done {
            StageReport::CacheResolved { spec, result } => match result {
                Ok(CachePlan::GitCache(cache_path)) => {
                    self.clone_queue.push_back(ReadyClone { spec, cache_path })
                }
                Ok(CachePlan::RegistryDownload) => {
                    self.ensure_download(spec.package.clone(), Some(spec))
                }
                Ok(CachePlan::DirectExtract) => self.direct_extract_queue.push_back(spec),
                Err(error) => self.complete_clone(spec.target, Err(error)),
            },
            StageReport::Download { package, result } => {
                let key = package.key();
                self.download_active.remove(&key);
                match result {
                    Ok(DownloadOutcome::Cached(cache_path)) => {
                        self.counts.reused += 1;
                        self.complete_download(key, Ok(cache_path));
                    }
                    Ok(DownloadOutcome::Bytes(bytes)) => {
                        self.extract_queue
                            .push_back(DownloadedPackage { package, bytes });
                    }
                    Err(error) => {
                        self.complete_download(key, Err(error));
                    }
                }
            }
            StageReport::Extract { key, result } => {
                self.extract_active.remove(&key);
                let path_result = match result {
                    Ok(ExtractOutcome::Extracted(path)) => {
                        self.counts.downloaded += 1;
                        Ok(path)
                    }
                    Ok(ExtractOutcome::Reused(path)) => {
                        self.counts.reused += 1;
                        Ok(path)
                    }
                    Err(e) => Err(e),
                };
                self.complete_download(key, path_result);
            }
            StageReport::DirectExtract { target, result } => {
                self.direct_extract_active.remove(&target);
                // A direct extract writes the package straight into node_modules
                // (no cloner step), so count it as a clone for the install summary
                // and finalize the clone for this target.
                let clone_result = result.inspect(|()| {
                    self.counts.cloned += 1;
                });
                self.complete_clone(target, clone_result);
            }
            StageReport::Clone { target, result } => {
                self.clone_active.remove(&target);
                let clone_result = match result {
                    Ok(fresh) => {
                        if fresh {
                            self.counts.cloned += 1;
                        }
                        Ok(())
                    }
                    Err(e) => Err(e),
                };
                self.complete_clone(target, clone_result);
            }
        }
    }

    fn complete_download(&mut self, key: String, result: Result<PathBuf, String>) {
        let waiters = self.fetch_waiters.remove(&key).unwrap_or_default();
        match result {
            Ok(cache_path) => {
                self.download_done.insert(key, cache_path.clone());
                for spec in waiters {
                    self.clone_queue.push_back(ReadyClone {
                        spec,
                        cache_path: cache_path.clone(),
                    });
                }
            }
            Err(error) => {
                for spec in waiters {
                    self.complete_clone(spec.target, Err(error.clone()));
                }
            }
        }
    }

    fn complete_clone(&mut self, target: PathBuf, result: Result<(), String>) {
        if result.is_ok() {
            self.clone_done.insert(target.clone());
        }

        if let Some(waiters) = self.clone_waiters.remove(&target) {
            for waiter in waiters {
                let _ = waiter.send(result.clone());
            }
        }

        if result.is_ok() {
            if let Some(children) = self.blocked_by_parent.remove(&target) {
                for child in children {
                    self.resolve_cache_for_clone(child);
                }
            }
        } else if let Some(children) = self.blocked_by_parent.remove(&target) {
            let error = format!("parent package {} failed to clone", target.display());
            for child in children {
                self.complete_clone(child.target, Err(error.clone()));
            }
        }
    }

    fn fail_pending(&mut self, message: &str) {
        for (_, waiters) in self.clone_waiters.drain() {
            for waiter in waiters {
                let _ = waiter.send(Err(message.to_string()));
            }
        }
    }
}

fn clone_concurrency_limit() -> usize {
    std::thread::available_parallelism()
        .map(|n| (n.get() * 2).clamp(4, 16))
        .unwrap_or(8)
}

fn extract_concurrency_limit() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().clamp(2, 8))
        .unwrap_or(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(name: &str, version: &str) -> PackageRef {
        PackageRef {
            name: name.to_string(),
            version: version.to_string(),
            tarball_url: format!("https://registry.npmjs.org/{name}/-/{name}-{version}.tgz"),
        }
    }

    fn clone_spec(name: &str, version: &str, target: &str) -> CloneSpec {
        CloneSpec {
            package: package(name, version),
            target: PathBuf::from(target),
            parent: None,
        }
    }

    fn state() -> SchedulerState {
        let (_tx, rx) = mpsc::unbounded_channel();
        SchedulerState::new(rx)
    }

    #[test]
    fn ensure_download_dedupes_inflight_package() {
        let mut state = state();
        let package = package("react", "18.2.0");
        let waiter = clone_spec("react", "18.2.0", "/tmp/project/node_modules/react");

        state.ensure_download(package.clone(), Some(waiter));
        state.ensure_download(package, None);

        assert_eq!(state.download_queue.len(), 1);
        assert_eq!(state.fetch_waiters["react@18.2.0"].len(), 1);
    }

    #[test]
    fn download_completion_releases_slot_and_queues_extract() {
        let mut state = state();
        let package = package("react", "18.2.0");
        let key = package.key();
        let waiter = clone_spec("react", "18.2.0", "/tmp/project/node_modules/react");

        state.download_active.insert(key.clone());
        state.fetch_waiters.insert(key.clone(), vec![waiter]);

        state.handle_report(StageReport::Download {
            package: package.clone(),
            result: Ok(DownloadOutcome::Bytes(Bytes::from_static(b"tgz"))),
        });

        assert!(!state.download_active.contains(&key));
        assert!(state.fetch_waiters.contains_key(&key));
        assert_eq!(state.extract_queue.len(), 1);
        assert_eq!(state.extract_queue[0].package.key(), key);
    }

    #[test]
    fn extract_completion_wakes_clone_waiters() {
        let mut state = state();
        let key = "react@18.2.0".to_string();
        let waiter = clone_spec("react", "18.2.0", "/tmp/project/node_modules/react");
        let cache_path = PathBuf::from("/tmp/cache/react/18.2.0");

        state.extract_active.insert(key.clone());
        state.fetch_waiters.insert(key.clone(), vec![waiter]);

        state.handle_report(StageReport::Extract {
            key: key.clone(),
            result: Ok(ExtractOutcome::Extracted(cache_path.clone())),
        });

        assert!(!state.extract_active.contains(&key));
        assert_eq!(state.download_done[&key], cache_path);
        assert_eq!(state.clone_queue.len(), 1);
    }

    #[tokio::test]
    async fn queue_clone_dedupes_inflight_target() {
        let mut state = state();
        let target = PathBuf::from("/tmp/project/node_modules/react");
        let spec = clone_spec("react", "18.2.0", target.to_string_lossy().as_ref());
        let (first, _first_rx) = oneshot::channel();
        let (second, _second_rx) = oneshot::channel();

        state.queue_clone(spec.clone(), Some(first));
        state.queue_clone(spec, Some(second));

        assert_eq!(state.clone_waiters[&target].len(), 2);
        assert_eq!(state.async_ops.len(), 1);
    }

    #[tokio::test]
    async fn queue_clone_blocks_child_until_parent_completes() {
        let mut state = state();
        let parent_target = PathBuf::from("/tmp/project/node_modules/@scope");
        let child_target = parent_target.join("child");
        let parent = clone_spec("scope", "1.0.0", parent_target.to_string_lossy().as_ref());
        let mut child = clone_spec("child", "1.0.0", child_target.to_string_lossy().as_ref());
        child.parent = Some(parent_target.clone());

        state.queue_clone(parent, None);
        let parent_ops = state.async_ops.len();
        state.queue_clone(child, None);

        assert_eq!(state.blocked_by_parent[&parent_target].len(), 1);
        assert_eq!(state.async_ops.len(), parent_ops);

        state.complete_clone(parent_target.clone(), Ok(()));

        assert!(!state.blocked_by_parent.contains_key(&parent_target));
        assert_eq!(state.async_ops.len(), parent_ops + 1);
    }

    #[tokio::test]
    async fn parent_clone_failure_wakes_blocked_children() {
        let mut state = state();
        let parent_target = PathBuf::from("/tmp/project/node_modules/@scope");
        let child_target = parent_target.join("child");
        let parent = clone_spec("scope", "1.0.0", parent_target.to_string_lossy().as_ref());
        let mut child = clone_spec("child", "1.0.0", child_target.to_string_lossy().as_ref());
        child.parent = Some(parent_target.clone());
        let (child_tx, child_rx) = oneshot::channel();

        state.queue_clone(parent, None);
        state.queue_clone(child, Some(child_tx));

        assert_eq!(state.blocked_by_parent[&parent_target].len(), 1);

        state.complete_clone(parent_target.clone(), Err("boom".to_string()));

        let error = child_rx.await.unwrap().unwrap_err();
        let parent_path = parent_target.to_string_lossy();
        assert!(error.contains("parent package"));
        assert!(error.contains(parent_path.as_ref()));
        assert!(!state.clone_waiters.contains_key(&child_target));
        assert!(!state.blocked_by_parent.contains_key(&parent_target));
    }
}
