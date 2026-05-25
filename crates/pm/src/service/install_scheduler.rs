use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use bytes::Bytes;
use futures::stream::{FuturesUnordered, StreamExt};
use tokio::sync::{mpsc, oneshot};
use utoo_ruborist::progress::{BuildEvent, EventReceiver};

use crate::util::cloner::{PackageClone, clone_package_sync};
use crate::util::downloader::{download_bytes, is_registry_tarball_url};
use crate::util::package_cache::{
    ExtractOutcome, extract_to_cache, registry_cache_lookup, resolve_seeded_cache_path,
};
use crate::util::user_config::get_manifests_concurrency_limit_sync;

/// Build event receiver that forwards install prefetch work to the scheduler.
pub(crate) struct InstallEventReceiver<R: EventReceiver> {
    scheduler: InstallScheduler,
    cwd: PathBuf,
    inner: R,
}

impl<R: EventReceiver> InstallEventReceiver<R> {
    pub(crate) fn new(inner: R, scheduler: InstallScheduler, cwd: PathBuf) -> Self {
        Self {
            scheduler,
            cwd,
            inner,
        }
    }
}

impl<R: EventReceiver> EventReceiver for InstallEventReceiver<R> {
    fn on_event(&self, event: BuildEvent<'_>) {
        self.inner.on_event(event);

        match event {
            BuildEvent::PackageResolved(info) if info.is_platform_compatible() => {
                let Some(tarball_url) = info.tarball_url else {
                    return;
                };
                self.scheduler.prefetch_download(
                    info.name.to_string(),
                    info.version.to_string(),
                    tarball_url.to_string(),
                );
            }
            BuildEvent::PackagePlaced {
                package,
                path,
                parent_path,
            } if package.is_platform_compatible() => {
                let Some(tarball_url) = package.tarball_url else {
                    return;
                };
                self.scheduler.prefetch_clone(
                    package.name.to_string(),
                    package.version.to_string(),
                    tarball_url.to_string(),
                    self.cwd.join(path),
                    parent_path.map(|p| self.cwd.join(p)),
                );
            }
            _ => {}
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
    PrefetchClone(CloneSpec),
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
        result: Result<Option<PathBuf>, String>,
    },
    Download {
        package: PackageRef,
        result: Result<DownloadOutcome, String>,
    },
    Extract {
        key: String,
        result: Result<ExtractOutcome, String>,
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
        if !is_registry_tarball_url(&tarball_url) {
            return;
        }
        let _ = self.tx.send(Command::PrefetchDownload(PackageRef {
            name,
            version,
            tarball_url,
        }));
    }

    pub(crate) fn prefetch_clone(
        &self,
        name: String,
        version: String,
        tarball_url: String,
        target: PathBuf,
        parent: Option<PathBuf>,
    ) {
        let _ = self.tx.send(Command::PrefetchClone(CloneSpec {
            package: PackageRef {
                name,
                version,
                tarball_url,
            },
            target,
            parent,
        }));
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
                        Some(Command::PrefetchClone(spec)) => {
                            self.queue_clone(spec, None);
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
                        Some(Err(e)) => tracing::warn!("Install scheduler worker failed: {e}"),
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
            && self.clone_queue.is_empty()
            && self.download_active.is_empty()
            && self.extract_active.is_empty()
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
            let result = resolve_seeded_cache_path(
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
        // cannot outrun CPU/disk extraction and pile up Bytes.
        while self.download_active.len() < self.download_limit
            && self.extract_backlog() < self.download_limit
        {
            let Some(package) = self.download_queue.pop_front() else {
                break;
            };
            let key = package.key();
            if self.download_done.contains_key(&key) || !self.download_active.insert(key.clone()) {
                continue;
            }

            self.async_ops.push(tokio::spawn(async move {
                let result = match registry_cache_lookup(&package.name, &package.version).await {
                    Ok(Some(cache_path)) => Ok(DownloadOutcome::Cached(cache_path)),
                    Ok(None) => download_bytes(&package.tarball_url)
                        .await
                        .map(DownloadOutcome::Bytes)
                        .map_err(|e| format!("{e:#}")),
                    Err(e) => Err(format!("{e:#}")),
                };
                StageReport::Download { package, result }
            }));
        }
    }

    fn extract_backlog(&self) -> usize {
        self.extract_queue.len() + self.extract_active.len()
    }

    fn pump_extracts(&mut self) {
        while self.extract_active.len() < self.extract_limit {
            let Some(downloaded) = self.extract_queue.pop_front() else {
                break;
            };
            let key = downloaded.package.key();
            if self.download_done.contains_key(&key) || !self.extract_active.insert(key.clone()) {
                continue;
            }

            self.async_ops.push(tokio::spawn(async move {
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
        while self.clone_active.len() < self.clone_limit {
            let Some(job) = self.clone_queue.pop_front() else {
                break;
            };
            let target = job.spec.target.clone();
            if self.clone_done.contains(&target) || !self.clone_active.insert(target.clone()) {
                continue;
            }

            let clone_done_tx = self.clone_done_tx.clone();
            rayon::spawn(move || {
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

    fn handle_report(&mut self, done: StageReport) {
        match done {
            StageReport::CacheResolved { spec, result } => match result {
                Ok(Some(cache_path)) => self.clone_queue.push_back(ReadyClone { spec, cache_path }),
                Ok(None) => self.ensure_download(spec.package.clone(), Some(spec)),
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
