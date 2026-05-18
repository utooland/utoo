use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use bytes::Bytes;
use futures::stream::{FuturesUnordered, StreamExt};
use tokio::sync::{mpsc, oneshot};
use utoo_ruborist::progress::{BuildEvent, EventReceiver};

use crate::util::cloner::{clone_count, clone_package_from_cache};
use crate::util::downloader::{
    download_bytes, download_stats, extract_to_cache, is_registry_tarball_url,
    registry_cache_lookup, resolve_seeded_cache_path,
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

pub(crate) fn print_summary() {
    tracing::debug!(
        "Install scheduler stats: downloaded={}, cloned={}",
        download_stats().downloaded,
        clone_count(),
    );
}

#[derive(Clone, Debug)]
struct PackageFetch {
    name: String,
    version: String,
    tarball_url: String,
}

impl PackageFetch {
    fn key(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }
}

#[derive(Clone, Debug)]
struct CloneSpec {
    package: PackageFetch,
    target: PathBuf,
    parent: Option<PathBuf>,
}

struct ReadyClone {
    spec: CloneSpec,
    cache_path: PathBuf,
}

struct DownloadedPackage {
    package: PackageFetch,
    bytes: Bytes,
}

type CloneResponder = oneshot::Sender<Result<(), String>>;

enum Command {
    PrefetchDownload(PackageFetch),
    PrefetchClone(CloneSpec),
    EnsureClone(CloneSpec, CloneResponder),
    Shutdown,
}

enum OpDone {
    SeededCache {
        spec: CloneSpec,
        result: Result<Option<PathBuf>, String>,
        elapsed: Option<Duration>,
    },
    Download {
        package: PackageFetch,
        result: Result<DownloadOutcome, String>,
        elapsed: Option<Duration>,
    },
    Extract {
        key: String,
        result: Result<PathBuf, String>,
        elapsed: Option<Duration>,
    },
    Clone {
        target: PathBuf,
        result: Result<(), String>,
        elapsed: Option<Duration>,
    },
}

enum DownloadOutcome {
    Cached(PathBuf),
    Bytes(Bytes),
}

#[derive(Clone)]
pub(crate) struct InstallScheduler {
    tx: mpsc::UnboundedSender<Command>,
}

pub(crate) struct InstallSchedulerHandle {
    scheduler: InstallScheduler,
    handle: tokio::task::JoinHandle<()>,
}

impl InstallSchedulerHandle {
    pub(crate) fn start() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let handle = tokio::spawn(async move {
            SchedulerState::new(rx).run().await;
        });
        Self {
            scheduler: InstallScheduler { tx },
            handle,
        }
    }

    pub(crate) fn scheduler(&self) -> InstallScheduler {
        self.scheduler.clone()
    }

    pub(crate) async fn shutdown(self) {
        let _ = self.scheduler.tx.send(Command::Shutdown);
        if let Err(e) = self.handle.await {
            tracing::warn!("Install scheduler task failed: {e}");
        }
    }
}

impl InstallScheduler {
    pub(crate) fn prefetch_download(&self, name: String, version: String, tarball_url: String) {
        if !is_registry_tarball_url(&tarball_url) {
            return;
        }
        let _ = self.tx.send(Command::PrefetchDownload(PackageFetch {
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
            package: PackageFetch {
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
                    package: PackageFetch {
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
    stats_enabled: bool,
    download_limit: usize,
    extract_limit: usize,
    clone_limit: usize,
    download_done: HashMap<String, PathBuf>,
    download_active: HashSet<String>,
    download_waiters: HashMap<String, Vec<CloneSpec>>,
    download_queue: VecDeque<PackageFetch>,
    extract_active: HashSet<String>,
    extract_queue: VecDeque<DownloadedPackage>,
    clone_done: HashSet<PathBuf>,
    clone_active: HashSet<PathBuf>,
    clone_waiters: HashMap<PathBuf, Vec<CloneResponder>>,
    blocked_by_parent: HashMap<PathBuf, Vec<CloneSpec>>,
    clone_queue: VecDeque<ReadyClone>,
    ops: FuturesUnordered<tokio::task::JoinHandle<OpDone>>,
    stats: SchedulerStats,
}

impl SchedulerState {
    fn new(rx: mpsc::UnboundedReceiver<Command>) -> Self {
        Self {
            rx,
            shutdown: false,
            stats_enabled: std::env::var_os("UTOO_INSTALL_SCHEDULER_STATS").is_some(),
            download_limit: get_manifests_concurrency_limit_sync().max(1),
            extract_limit: extract_concurrency_limit(),
            clone_limit: clone_concurrency_limit(),
            download_done: HashMap::new(),
            download_active: HashSet::new(),
            download_waiters: HashMap::new(),
            download_queue: VecDeque::new(),
            extract_active: HashSet::new(),
            extract_queue: VecDeque::new(),
            clone_done: HashSet::new(),
            clone_active: HashSet::new(),
            clone_waiters: HashMap::new(),
            blocked_by_parent: HashMap::new(),
            clone_queue: VecDeque::new(),
            ops: FuturesUnordered::new(),
            stats: SchedulerStats::default(),
        }
    }

    async fn run(mut self) {
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
                done = self.ops.next(), if !self.ops.is_empty() => {
                    match done {
                        Some(Ok(done)) => self.handle_done(done),
                        Some(Err(e)) => tracing::warn!("Install scheduler worker failed: {e}"),
                        None => {}
                    }
                }
            }
        }

        if self.stats_enabled {
            self.stats.log();
        }
        self.fail_pending("install scheduler stopped before work completed");
    }

    fn timer(&self) -> Option<Instant> {
        self.stats_enabled.then(Instant::now)
    }

    fn is_idle(&self) -> bool {
        self.download_queue.is_empty()
            && self.extract_queue.is_empty()
            && self.clone_queue.is_empty()
            && self.download_active.is_empty()
            && self.extract_active.is_empty()
            && self.clone_active.is_empty()
            && self.ops.is_empty()
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
        let timer = self.timer();
        self.ops.push(tokio::spawn(async move {
            let result = resolve_seeded_cache_path(
                &task_spec.package.name,
                &task_spec.package.version,
                &task_spec.package.tarball_url,
            )
            .await
            .map_err(|e| format!("{e:#}"));
            OpDone::SeededCache {
                spec,
                result,
                elapsed: timer.map(|started| started.elapsed()),
            }
        }));
    }

    fn ensure_download(&mut self, package: PackageFetch, waiter: Option<CloneSpec>) {
        let key = package.key();
        if let Some(cache_path) = self.download_done.get(&key).cloned() {
            if let Some(spec) = waiter {
                self.clone_queue.push_back(ReadyClone { spec, cache_path });
            }
            return;
        }

        if let Some(waiters) = self.download_waiters.get_mut(&key) {
            if let Some(spec) = waiter {
                waiters.push(spec);
            }
            return;
        }

        self.download_waiters
            .insert(key, waiter.into_iter().collect());
        self.download_queue.push_back(package);
    }

    fn pump_downloads(&mut self) {
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

            let timer = self.timer();
            self.ops.push(tokio::spawn(async move {
                let result = match registry_cache_lookup(&package.name, &package.version).await {
                    Ok(Some(cache_path)) => Ok(DownloadOutcome::Cached(cache_path)),
                    Ok(None) => download_bytes(&package.tarball_url)
                        .await
                        .map(DownloadOutcome::Bytes)
                        .map_err(|e| format!("{e:#}")),
                    Err(e) => Err(format!("{e:#}")),
                };
                OpDone::Download {
                    package,
                    result,
                    elapsed: timer.map(|started| started.elapsed()),
                }
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

            let timer = self.timer();
            self.ops.push(tokio::spawn(async move {
                let result = extract_to_cache(
                    &downloaded.package.name,
                    &downloaded.package.version,
                    downloaded.bytes,
                )
                .await
                .map_err(|e| format!("{e:#}"));
                OpDone::Extract {
                    key,
                    result,
                    elapsed: timer.map(|started| started.elapsed()),
                }
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

            let timer = self.timer();
            self.ops.push(tokio::spawn(async move {
                let result = clone_package_from_cache(
                    &job.spec.package.name,
                    &job.spec.package.version,
                    &job.spec.package.tarball_url,
                    &job.cache_path,
                    &job.spec.target,
                )
                .await
                .map_err(|e| format!("{e:#}"));
                OpDone::Clone {
                    target,
                    result,
                    elapsed: timer.map(|started| started.elapsed()),
                }
            }));
        }
    }

    fn handle_done(&mut self, done: OpDone) {
        match done {
            OpDone::SeededCache {
                spec,
                result,
                elapsed,
            } => {
                self.stats.seeded_cache.record(elapsed);
                match result {
                    Ok(Some(cache_path)) => {
                        self.stats.seeded_cache_hits += 1;
                        self.clone_queue.push_back(ReadyClone { spec, cache_path });
                    }
                    Ok(None) => {
                        self.stats.seeded_cache_misses += 1;
                        self.ensure_download(spec.package.clone(), Some(spec));
                    }
                    Err(error) => self.complete_clone(spec.target, Err(error)),
                }
            }
            OpDone::Download {
                package,
                result,
                elapsed,
            } => {
                self.stats.download.record(elapsed);
                let key = package.key();
                self.download_active.remove(&key);
                match result {
                    Ok(DownloadOutcome::Cached(cache_path)) => {
                        self.stats.download_cache_hits += 1;
                        self.complete_download(key, Ok(cache_path));
                    }
                    Ok(DownloadOutcome::Bytes(bytes)) => {
                        self.stats.download_network += 1;
                        self.stats.downloaded_bytes += bytes.len() as u64;
                        self.extract_queue
                            .push_back(DownloadedPackage { package, bytes });
                    }
                    Err(error) => {
                        self.complete_download(key, Err(error));
                    }
                }
            }
            OpDone::Extract {
                key,
                result,
                elapsed,
            } => {
                self.stats.extract.record(elapsed);
                self.extract_active.remove(&key);
                self.complete_download(key, result);
            }
            OpDone::Clone {
                target,
                result,
                elapsed,
            } => {
                self.stats.clone.record(elapsed);
                self.clone_active.remove(&target);
                self.complete_clone(target, result);
            }
        }
    }

    fn complete_download(&mut self, key: String, result: Result<PathBuf, String>) {
        let waiters = self.download_waiters.remove(&key).unwrap_or_default();
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

#[derive(Default)]
struct SchedulerStats {
    seeded_cache: StageStats,
    download: StageStats,
    extract: StageStats,
    clone: StageStats,
    seeded_cache_hits: usize,
    seeded_cache_misses: usize,
    download_cache_hits: usize,
    download_network: usize,
    downloaded_bytes: u64,
}

impl SchedulerStats {
    fn log(&self) {
        tracing::info!(
            target: "utoo::install_scheduler",
            seeded_cache_count = self.seeded_cache.count,
            seeded_cache_ms = self.seeded_cache.total.as_millis(),
            seeded_cache_hits = self.seeded_cache_hits,
            seeded_cache_misses = self.seeded_cache_misses,
            download_count = self.download.count,
            download_ms = self.download.total.as_millis(),
            download_cache_hits = self.download_cache_hits,
            download_network = self.download_network,
            downloaded_mb = self.downloaded_bytes / 1024 / 1024,
            extract_count = self.extract.count,
            extract_ms = self.extract.total.as_millis(),
            clone_count = self.clone.count,
            clone_ms = self.clone.total.as_millis(),
            "install scheduler stats"
        );
    }
}

#[derive(Default)]
struct StageStats {
    count: usize,
    total: Duration,
}

impl StageStats {
    fn record(&mut self, elapsed: Option<Duration>) {
        if let Some(elapsed) = elapsed {
            self.count += 1;
            self.total += elapsed;
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

    fn package(name: &str, version: &str) -> PackageFetch {
        PackageFetch {
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
        assert_eq!(state.download_waiters["react@18.2.0"].len(), 1);
    }

    #[test]
    fn download_completion_releases_slot_and_queues_extract() {
        let mut state = state();
        let package = package("react", "18.2.0");
        let key = package.key();
        let waiter = clone_spec("react", "18.2.0", "/tmp/project/node_modules/react");

        state.download_active.insert(key.clone());
        state.download_waiters.insert(key.clone(), vec![waiter]);

        state.handle_done(OpDone::Download {
            package: package.clone(),
            result: Ok(DownloadOutcome::Bytes(Bytes::from_static(b"tgz"))),
            elapsed: None,
        });

        assert!(!state.download_active.contains(&key));
        assert!(state.download_waiters.contains_key(&key));
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
        state.download_waiters.insert(key.clone(), vec![waiter]);

        state.handle_done(OpDone::Extract {
            key: key.clone(),
            result: Ok(cache_path.clone()),
            elapsed: None,
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
        assert_eq!(state.ops.len(), 1);
    }
}
