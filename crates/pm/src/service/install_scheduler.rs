use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use anyhow::{Context, Result, anyhow};
use bytes::Bytes;
use futures::stream::{FuturesUnordered, StreamExt};
use tokio::sync::{mpsc, oneshot};
use utoo_ruborist::progress::{BuildEvent, EventReceiver};

use crate::util::cloner::{clone_count, clone_package_from_cache_indexed_sync};
use crate::util::downloader::{
    RegistryCacheEntry, download_bytes, download_stats, extract_to_cache_indexed_sync,
    is_registry_tarball_url, registry_cache_lookup_sync, resolve_seeded_cache_path,
};
use crate::util::user_config::get_manifests_concurrency_limit_sync;

// Keep clone completions batched enough to reduce scheduler wakeups, while
// avoiding long serial hardlink runs inside one rayon worker.
const CLONE_BATCH_LIMIT: usize = 3;
const MIN_INSTALL_DOWNLOAD_CONCURRENCY_LIMIT: usize = 128;

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
    cache: RegistryCacheEntry,
}

struct DownloadedPackage {
    package: PackageFetch,
    bytes: Bytes,
}

type CloneResponder = oneshot::Sender<Result<(), String>>;
type SchedulerOp = Pin<Box<dyn Future<Output = OpDone> + Send>>;

enum Command {
    PrefetchDownload(PackageFetch),
    PrefetchClone(CloneSpec),
    EnsureClone(CloneSpec, CloneResponder),
    Shutdown,
}

enum OpDone {
    SeededCache {
        spec: CloneSpec,
        result: Result<Option<RegistryCacheEntry>, String>,
    },
    Download {
        package: PackageFetch,
        result: Result<DownloadOutcome, String>,
    },
    Extract {
        key: String,
        result: Result<RegistryCacheEntry, String>,
    },
    CloneBatch {
        results: Vec<(PathBuf, Result<(), String>)>,
    },
}

enum DownloadOutcome {
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
    done_tx: mpsc::UnboundedSender<OpDone>,
    done_rx: mpsc::UnboundedReceiver<OpDone>,
    shutdown: bool,
    download_limit: usize,
    extract_limit: usize,
    clone_worker_limit: usize,
    download_done: HashMap<String, RegistryCacheEntry>,
    download_active: HashSet<String>,
    download_waiters: HashMap<String, Vec<CloneSpec>>,
    download_queue: VecDeque<PackageFetch>,
    extract_active: HashSet<String>,
    extract_queue: VecDeque<DownloadedPackage>,
    clone_done: HashSet<PathBuf>,
    clone_active: HashSet<PathBuf>,
    clone_workers_active: usize,
    clone_waiters: HashMap<PathBuf, Vec<CloneResponder>>,
    blocked_by_parent: HashMap<PathBuf, Vec<CloneSpec>>,
    clone_queue: VecDeque<ReadyClone>,
    ops: FuturesUnordered<SchedulerOp>,
}

impl SchedulerState {
    fn new(rx: mpsc::UnboundedReceiver<Command>) -> Self {
        let (done_tx, done_rx) = mpsc::unbounded_channel();
        let clone_limit = clone_concurrency_limit();
        Self {
            rx,
            done_tx,
            done_rx,
            shutdown: false,
            download_limit: install_download_concurrency_limit(),
            extract_limit: extract_concurrency_limit(),
            clone_worker_limit: clone_worker_limit(clone_limit),
            download_done: HashMap::new(),
            download_active: HashSet::new(),
            download_waiters: HashMap::new(),
            download_queue: VecDeque::new(),
            extract_active: HashSet::new(),
            extract_queue: VecDeque::new(),
            clone_done: HashSet::new(),
            clone_active: HashSet::new(),
            clone_workers_active: 0,
            clone_waiters: HashMap::new(),
            blocked_by_parent: HashMap::new(),
            clone_queue: VecDeque::new(),
            ops: FuturesUnordered::new(),
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
                    if let Some(done) = done {
                        self.handle_done(done);
                    }
                }
                done = self.done_rx.recv() => {
                    if let Some(done) = done {
                        self.handle_done(done);
                    }
                }
            }
        }

        self.fail_pending("install scheduler stopped before work completed");
    }

    fn is_idle(&self) -> bool {
        self.download_queue.is_empty()
            && self.extract_queue.is_empty()
            && self.clone_queue.is_empty()
            && self.download_active.is_empty()
            && self.extract_active.is_empty()
            && self.clone_active.is_empty()
            && self.clone_workers_active == 0
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

        if self.has_registry_download_state(&spec.package) {
            self.ensure_download(spec.package.clone(), Some(spec));
            return;
        }

        self.resolve_cache_for_clone(spec);
    }

    fn has_registry_download_state(&self, package: &PackageFetch) -> bool {
        let key = package.key();
        self.download_done.contains_key(&key) || self.download_waiters.contains_key(&key)
    }

    fn resolve_cache_for_clone(&mut self, spec: CloneSpec) {
        let task_spec = spec.clone();
        self.ops.push(Box::pin(async move {
            let result = resolve_seeded_cache_path(
                &task_spec.package.name,
                &task_spec.package.version,
                &task_spec.package.tarball_url,
            )
            .await
            .map(|maybe_path| maybe_path.map(|path| RegistryCacheEntry { path, index: None }))
            .map_err(|e| format!("{e:#}"));
            OpDone::SeededCache { spec, result }
        }));
    }

    fn ensure_download(&mut self, package: PackageFetch, waiter: Option<CloneSpec>) {
        let key = package.key();
        if let Some(cache) = self.download_done.get(&key).cloned() {
            if let Some(spec) = waiter {
                self.clone_queue.push_back(ReadyClone { spec, cache });
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
        self.pump_downloads_with(|package| {
            registry_cache_lookup_sync(&package.name, &package.version)
        });
    }

    fn pump_downloads_with(&mut self, cache_lookup: impl Fn(&PackageFetch) -> Option<PathBuf>) {
        while self.download_active.len() < self.download_limit
            && self.extract_backlog() < self.download_limit
        {
            let Some(package) = self.download_queue.pop_front() else {
                break;
            };
            let key = package.key();
            if self.download_done.contains_key(&key) || self.download_active.contains(&key) {
                continue;
            }

            if let Some(cache_path) = cache_lookup(&package) {
                self.complete_download(
                    key,
                    Ok(RegistryCacheEntry {
                        path: cache_path,
                        index: None,
                    }),
                );
                continue;
            }

            self.download_active.insert(key.clone());
            self.ops.push(Box::pin(async move {
                let result = download_bytes(&package.tarball_url)
                    .await
                    .map(DownloadOutcome::Bytes)
                    .map_err(|e| format!("{e:#}"));
                OpDone::Download { package, result }
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

            let done_tx = self.done_tx.clone();
            rayon::spawn(move || {
                let result = extract_to_cache_indexed_sync(
                    &downloaded.package.name,
                    &downloaded.package.version,
                    downloaded.bytes,
                )
                .map_err(|e| format!("{e:#}"));
                let _ = done_tx.send(OpDone::Extract { key, result });
            });
        }
    }

    fn pump_clones(&mut self) {
        while self.clone_workers_active < self.clone_worker_limit {
            if !self.spawn_clone_batch() {
                break;
            }
        }
    }

    fn spawn_clone_batch(&mut self) -> bool {
        let batch = self.take_clone_batch();
        if batch.is_empty() {
            return false;
        }

        self.clone_workers_active += 1;
        let done_tx = self.done_tx.clone();
        rayon::spawn(move || {
            let results = batch
                .into_iter()
                .map(|(target, job)| {
                    let result = clone_package_from_cache_indexed_sync(
                        &job.spec.package.name,
                        &job.spec.package.version,
                        &job.spec.package.tarball_url,
                        &job.cache.path,
                        &job.spec.target,
                        job.cache.index.as_ref(),
                    )
                    .map_err(|e| format!("{e:#}"));
                    (target, result)
                })
                .collect();
            let _ = done_tx.send(OpDone::CloneBatch { results });
        });

        true
    }

    fn take_clone_batch(&mut self) -> Vec<(PathBuf, ReadyClone)> {
        let mut batch = Vec::new();
        while batch.len() < CLONE_BATCH_LIMIT {
            let Some(job) = self.pop_next_clone_job() else {
                break;
            };
            let target = job.spec.target.clone();
            if self.clone_done.contains(&target) || !self.clone_active.insert(target.clone()) {
                continue;
            }

            // Parent clones unblock nested dependency placement. Keep them out
            // of multi-package batches so their children can enter the queue as
            // soon as the parent materializes.
            let unblocks_children = self.blocked_by_parent.contains_key(&target);
            batch.push((target, job));
            if unblocks_children {
                break;
            }
        }
        batch
    }

    fn pop_next_clone_job(&mut self) -> Option<ReadyClone> {
        let unblocker_index = if self.blocked_by_parent.is_empty() {
            None
        } else {
            self.clone_queue
                .iter()
                .position(|job| self.blocked_by_parent.contains_key(&job.spec.target))
        };
        match unblocker_index {
            Some(index) => self.clone_queue.remove(index),
            None => self.clone_queue.pop_front(),
        }
    }

    fn handle_done(&mut self, done: OpDone) {
        match done {
            OpDone::SeededCache { spec, result } => match result {
                Ok(Some(cache)) => self.clone_queue.push_back(ReadyClone { spec, cache }),
                Ok(None) => self.ensure_download(spec.package.clone(), Some(spec)),
                Err(error) => self.complete_clone(spec.target, Err(error)),
            },
            OpDone::Download { package, result } => {
                let key = package.key();
                self.download_active.remove(&key);
                match result {
                    Ok(DownloadOutcome::Bytes(bytes)) => {
                        self.extract_queue
                            .push_back(DownloadedPackage { package, bytes });
                    }
                    Err(error) => {
                        self.complete_download(key, Err(error));
                    }
                }
            }
            OpDone::Extract { key, result } => {
                self.extract_active.remove(&key);
                self.complete_download(key, result);
            }
            OpDone::CloneBatch { results } => {
                self.clone_workers_active = self.clone_workers_active.saturating_sub(1);
                for (target, result) in results {
                    self.clone_active.remove(&target);
                    self.complete_clone(target, result);
                }
            }
        }
    }

    fn complete_download(&mut self, key: String, result: Result<RegistryCacheEntry, String>) {
        let waiters = self.download_waiters.remove(&key).unwrap_or_default();
        match result {
            Ok(cache) => {
                self.download_done.insert(key, cache.clone());
                for spec in waiters {
                    self.clone_queue.push_back(ReadyClone {
                        spec,
                        cache: cache.clone(),
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

fn clone_worker_limit(clone_limit: usize) -> usize {
    clone_limit
        .saturating_div(2)
        .saturating_add(2)
        .clamp(1, clone_limit.max(1))
}

fn install_download_concurrency_limit() -> usize {
    get_manifests_concurrency_limit_sync().max(MIN_INSTALL_DOWNLOAD_CONCURRENCY_LIMIT)
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

    fn cache_entry(path: PathBuf) -> RegistryCacheEntry {
        RegistryCacheEntry { path, index: None }
    }

    fn ready_clone(name: &str, version: &str, target: &str) -> ReadyClone {
        ReadyClone {
            spec: clone_spec(name, version, target),
            cache: cache_entry(PathBuf::from(format!("/tmp/cache/{name}/{version}"))),
        }
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
        });

        assert!(!state.download_active.contains(&key));
        assert!(state.download_waiters.contains_key(&key));
        assert_eq!(state.extract_queue.len(), 1);
        assert_eq!(state.extract_queue[0].package.key(), key);
    }

    #[test]
    fn download_cache_hit_completes_inline_without_worker() {
        let mut state = state();
        let package = package("react", "18.2.0");
        let key = package.key();
        let cache_path = PathBuf::from("/tmp/cache/react/18.2.0");
        let waiter = clone_spec("react", "18.2.0", "/tmp/project/node_modules/react");

        state.ensure_download(package.clone(), Some(waiter));
        state.pump_downloads_with(|queued| {
            assert_eq!(queued.key(), key);
            Some(cache_path.clone())
        });

        assert!(state.download_queue.is_empty());
        assert!(state.download_active.is_empty());
        assert!(state.ops.is_empty());
        assert_eq!(state.download_done[&key].path, cache_path);
        assert_eq!(state.clone_queue.len(), 1);
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
            result: Ok(cache_entry(cache_path.clone())),
        });

        assert!(!state.extract_active.contains(&key));
        assert_eq!(state.download_done[&key].path, cache_path);
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

    #[test]
    fn queue_clone_attaches_to_known_registry_download() {
        let mut state = state();
        let package = package("react", "18.2.0");
        let target = PathBuf::from("/tmp/project/node_modules/react");
        let spec = clone_spec("react", "18.2.0", target.to_string_lossy().as_ref());

        state.ensure_download(package, None);
        state.queue_clone(spec, None);

        assert_eq!(state.download_queue.len(), 1);
        assert_eq!(state.download_waiters["react@18.2.0"].len(), 1);
        assert!(state.ops.is_empty());
    }

    #[test]
    fn queue_clone_reuses_completed_registry_download() {
        let mut state = state();
        let target = PathBuf::from("/tmp/project/node_modules/react");
        let spec = clone_spec("react", "18.2.0", target.to_string_lossy().as_ref());
        let cache_path = PathBuf::from("/tmp/cache/react/18.2.0");

        state
            .download_done
            .insert("react@18.2.0".to_string(), cache_entry(cache_path.clone()));
        state.queue_clone(spec, None);

        assert_eq!(state.clone_queue.len(), 1);
        assert_eq!(state.clone_queue[0].cache.path, cache_path);
        assert!(state.ops.is_empty());
    }

    #[test]
    fn clone_worker_limit_batches_clone_completion_without_serializing_all_clones() {
        assert_eq!(clone_worker_limit(1), 1);
        assert_eq!(clone_worker_limit(4), 4);
        assert_eq!(clone_worker_limit(8), 6);
        assert_eq!(clone_worker_limit(16), 10);
    }

    #[test]
    fn install_download_concurrency_has_experiment_floor() {
        assert!(install_download_concurrency_limit() >= MIN_INSTALL_DOWNLOAD_CONCURRENCY_LIMIT);
    }

    #[test]
    fn clone_batch_prioritizes_parent_unblockers() {
        let mut state = state();
        let leaf = ready_clone("leaf", "1.0.0", "/tmp/project/node_modules/leaf");
        let parent = ready_clone("parent", "1.0.0", "/tmp/project/node_modules/parent");
        let child = clone_spec(
            "child",
            "1.0.0",
            "/tmp/project/node_modules/parent/node_modules/child",
        );
        let parent_target = parent.spec.target.clone();

        state.clone_queue.push_back(leaf);
        state.clone_queue.push_back(parent);
        state
            .blocked_by_parent
            .insert(parent_target.clone(), vec![child]);

        let batch = state.take_clone_batch();

        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].0, parent_target);
        assert!(state.clone_active.contains(&parent_target));
        assert_eq!(state.clone_queue.len(), 1);
        assert_eq!(
            state.clone_queue[0].spec.target,
            PathBuf::from("/tmp/project/node_modules/leaf")
        );
    }

    #[test]
    fn clone_batch_keeps_batched_leaf_clones() {
        let mut state = state();
        state
            .clone_queue
            .push_back(ready_clone("a", "1.0.0", "/tmp/project/node_modules/a"));
        state
            .clone_queue
            .push_back(ready_clone("b", "1.0.0", "/tmp/project/node_modules/b"));
        state
            .clone_queue
            .push_back(ready_clone("c", "1.0.0", "/tmp/project/node_modules/c"));
        state
            .clone_queue
            .push_back(ready_clone("d", "1.0.0", "/tmp/project/node_modules/d"));

        let batch = state.take_clone_batch();

        assert_eq!(batch.len(), CLONE_BATCH_LIMIT);
        assert_eq!(state.clone_queue.len(), 1);
    }
}
