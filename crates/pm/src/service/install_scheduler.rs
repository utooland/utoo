use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use bytes::Bytes;
use futures::stream::{FuturesUnordered, StreamExt};
use tokio::sync::{mpsc, oneshot};

use crate::util::cloner::clone_package_from_cache;
use crate::util::downloader::{
    download_bytes, extract_to_cache, registry_cache_lookup, resolve_seeded_cache_path,
};
use crate::util::user_config::get_manifests_concurrency_limit_sync;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct PackageKey(String);

#[derive(Clone, Debug)]
struct PackageFetch {
    name: String,
    version: String,
    tarball_url: String,
}

impl PackageFetch {
    fn key(&self) -> PackageKey {
        PackageKey(format!("{}@{}", self.name, self.version))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct InstallCloneRequest {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) tarball_url: String,
    pub(crate) target: PathBuf,
    pub(crate) parent: Option<PathBuf>,
}

impl InstallCloneRequest {
    fn package(&self) -> PackageFetch {
        PackageFetch {
            name: self.name.clone(),
            version: self.version.clone(),
            tarball_url: self.tarball_url.clone(),
        }
    }
}

#[derive(Debug)]
struct ReadyClone {
    request: InstallCloneRequest,
    cache_path: PathBuf,
}

#[derive(Debug)]
struct DownloadedPackage {
    package: PackageFetch,
    bytes: Bytes,
}

type CloneResponder = oneshot::Sender<Result<(), String>>;

const CLONE_CONCURRENCY_PER_CPU: usize = 2;
const MIN_CLONE_CONCURRENCY: usize = 4;
const MAX_CLONE_CONCURRENCY: usize = 16;
const DEFAULT_CLONE_CONCURRENCY: usize = 8;

#[cfg(windows)]
fn clone_key(target: &Path) -> PathBuf {
    target.components().collect()
}

#[cfg(not(windows))]
fn clone_key(target: &Path) -> PathBuf {
    target.to_path_buf()
}

enum Command {
    EnsureClone(InstallCloneRequest, CloneResponder),
    PrefetchClone(InstallCloneRequest),
    PrefetchDownload(PackageFetch),
    Shutdown,
}

enum OpDone {
    SeededCache {
        request: InstallCloneRequest,
        result: Result<Option<PathBuf>, String>,
    },
    Download {
        package: PackageFetch,
        result: Result<DownloadOutcome, String>,
    },
    Extract {
        key: PackageKey,
        result: Result<PathBuf, String>,
    },
    Clone {
        target: PathBuf,
        result: Result<(), String>,
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
    pub(crate) async fn ensure_clone(&self, request: InstallCloneRequest) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Command::EnsureClone(request, tx))
            .map_err(|_| anyhow!("install scheduler stopped"))?;
        rx.await
            .context("install scheduler stopped before clone completed")?
            .map_err(anyhow::Error::msg)
    }

    pub(crate) fn prefetch_clone(&self, request: InstallCloneRequest) -> Result<()> {
        self.tx
            .send(Command::PrefetchClone(request))
            .map_err(|_| anyhow!("install scheduler stopped"))
    }

    pub(crate) fn prefetch_download(
        &self,
        name: String,
        version: String,
        tarball_url: String,
    ) -> Result<()> {
        self.tx
            .send(Command::PrefetchDownload(PackageFetch {
                name,
                version,
                tarball_url,
            }))
            .map_err(|_| anyhow!("install scheduler stopped"))
    }
}

struct SchedulerState {
    rx: mpsc::UnboundedReceiver<Command>,
    shutdown: bool,
    download_limit: usize,
    extract_limit: usize,
    clone_limit: usize,
    download_done: HashMap<PackageKey, PathBuf>,
    download_active: HashSet<PackageKey>,
    download_waiters: HashMap<PackageKey, Vec<InstallCloneRequest>>,
    download_queue: VecDeque<PackageFetch>,
    extract_active: HashSet<PackageKey>,
    extract_queue: VecDeque<DownloadedPackage>,
    clone_done: HashSet<PathBuf>,
    clone_active: HashSet<PathBuf>,
    clone_waiters: HashMap<PathBuf, Vec<CloneResponder>>,
    blocked_by_parent: HashMap<PathBuf, Vec<InstallCloneRequest>>,
    clone_queue: VecDeque<ReadyClone>,
    ops: FuturesUnordered<tokio::task::JoinHandle<OpDone>>,
}

impl SchedulerState {
    fn new(rx: mpsc::UnboundedReceiver<Command>) -> Self {
        Self {
            rx,
            shutdown: false,
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
                        Some(Command::EnsureClone(request, responder)) => {
                            self.queue_clone(request, Some(responder));
                        }
                        Some(Command::PrefetchClone(request)) => {
                            self.queue_clone(request, None);
                        }
                        Some(Command::PrefetchDownload(package)) => {
                            self.ensure_download(package, None);
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

        self.fail_pending("install scheduler stopped before work completed");
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

    fn queue_clone(&mut self, request: InstallCloneRequest, responder: Option<CloneResponder>) {
        let target = clone_key(&request.target);
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

        if let Some(parent) = &request.parent {
            let parent = clone_key(parent);
            if self.clone_waiters.contains_key(&parent) && !self.clone_done.contains(&parent) {
                self.blocked_by_parent
                    .entry(parent)
                    .or_default()
                    .push(request);
                return;
            }
        }

        self.resolve_cache_for_clone(request);
    }

    fn resolve_cache_for_clone(&mut self, request: InstallCloneRequest) {
        let task_request = request.clone();
        self.ops.push(tokio::spawn(async move {
            let result = resolve_seeded_cache_path(
                &task_request.name,
                &task_request.version,
                &task_request.tarball_url,
            )
            .await
            .map_err(|e| format!("{e:#}"));
            OpDone::SeededCache { request, result }
        }));
    }

    fn ensure_download(&mut self, package: PackageFetch, waiter: Option<InstallCloneRequest>) {
        let key = package.key();
        if let Some(cache_path) = self.download_done.get(&key).cloned() {
            if let Some(request) = waiter {
                self.clone_queue.push_back(ReadyClone {
                    request,
                    cache_path,
                });
            }
            return;
        }

        if let Some(waiters) = self.download_waiters.get_mut(&key) {
            if let Some(request) = waiter {
                waiters.push(request);
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

            self.ops.push(tokio::spawn(async move {
                let result = match registry_cache_lookup(&package.name, &package.version).await {
                    Ok(Some(cache_path)) => Ok(DownloadOutcome::Cached(cache_path)),
                    Ok(None) => download_bytes(&package.tarball_url)
                        .await
                        .map(DownloadOutcome::Bytes)
                        .map_err(|e| format!("{e:#}")),
                    Err(e) => Err(format!("{e:#}")),
                };
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

            self.ops.push(tokio::spawn(async move {
                let result = extract_to_cache(
                    &downloaded.package.name,
                    &downloaded.package.version,
                    downloaded.bytes,
                )
                .await
                .map_err(|e| format!("{e:#}"));
                OpDone::Extract { key, result }
            }));
        }
    }

    fn pump_clones(&mut self) {
        while self.clone_active.len() < self.clone_limit {
            let Some(job) = self.clone_queue.pop_front() else {
                break;
            };
            let target = clone_key(&job.request.target);
            if self.clone_done.contains(&target) || !self.clone_active.insert(target.clone()) {
                continue;
            }

            self.ops.push(tokio::spawn(async move {
                let result = clone_package_from_cache(
                    &job.request.name,
                    &job.request.version,
                    &job.request.tarball_url,
                    &job.cache_path,
                    &job.request.target,
                )
                .await
                .map_err(|e| format!("{e:#}"));
                OpDone::Clone { target, result }
            }));
        }
    }

    fn handle_done(&mut self, done: OpDone) {
        match done {
            OpDone::SeededCache { request, result } => match result {
                Ok(Some(cache_path)) => self.clone_queue.push_back(ReadyClone {
                    request,
                    cache_path,
                }),
                Ok(None) => self.ensure_download(request.package(), Some(request)),
                Err(error) => self.complete_clone(clone_key(&request.target), Err(error)),
            },
            OpDone::Download { package, result } => {
                let key = package.key();
                self.download_active.remove(&key);
                match result {
                    Ok(DownloadOutcome::Cached(cache_path)) => {
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
            OpDone::Extract { key, result } => {
                self.extract_active.remove(&key);
                self.complete_download(key, result);
            }
            OpDone::Clone { target, result } => {
                self.clone_active.remove(&target);
                self.complete_clone(target, result);
            }
        }
    }

    fn complete_download(&mut self, key: PackageKey, result: Result<PathBuf, String>) {
        let waiters = self.download_waiters.remove(&key).unwrap_or_default();
        match result {
            Ok(cache_path) => {
                self.download_done.insert(key, cache_path.clone());
                for request in waiters {
                    self.clone_queue.push_back(ReadyClone {
                        request,
                        cache_path: cache_path.clone(),
                    });
                }
            }
            Err(error) => {
                for request in waiters {
                    self.complete_clone(clone_key(&request.target), Err(error.clone()));
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

        match (result.is_ok(), self.blocked_by_parent.remove(&target)) {
            (true, Some(children)) => {
                for child in children {
                    self.resolve_cache_for_clone(child);
                }
            }
            (false, Some(children)) => {
                let error = format!("parent package {} failed to clone", target.display());
                for child in children {
                    self.complete_clone(clone_key(&child.target), Err(error.clone()));
                }
            }
            (_, None) => {}
        }
    }

    fn fail_pending(&mut self, message: &str) {
        for waiters in self.clone_waiters.drain().map(|(_, waiters)| waiters) {
            for waiter in waiters {
                let _ = waiter.send(Err(message.to_string()));
            }
        }
    }
}

fn clone_concurrency_limit() -> usize {
    std::thread::available_parallelism()
        .map(|n| {
            (n.get() * CLONE_CONCURRENCY_PER_CPU)
                .clamp(MIN_CLONE_CONCURRENCY, MAX_CLONE_CONCURRENCY)
        })
        .unwrap_or(DEFAULT_CLONE_CONCURRENCY)
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

    fn clone_request(name: &str, version: &str, target: &str) -> InstallCloneRequest {
        InstallCloneRequest {
            name: name.to_string(),
            version: version.to_string(),
            tarball_url: format!("https://registry.npmjs.org/{name}/-/{name}-{version}.tgz"),
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
        let waiter = clone_request("react", "18.2.0", "/tmp/project/node_modules/react");

        state.ensure_download(package.clone(), Some(waiter));
        state.ensure_download(package, None);

        assert_eq!(state.download_queue.len(), 1);
        assert_eq!(
            state.download_waiters[&PackageKey("react@18.2.0".into())].len(),
            1
        );
    }

    #[test]
    fn download_completion_releases_slot_and_queues_extract() {
        let mut state = state();
        let package = package("react", "18.2.0");
        let key = package.key();
        let waiter = clone_request("react", "18.2.0", "/tmp/project/node_modules/react");

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
    fn extract_completion_wakes_clone_waiters() {
        let mut state = state();
        let key = PackageKey("react@18.2.0".to_string());
        let waiter = clone_request("react", "18.2.0", "/tmp/project/node_modules/react");
        let cache_path = PathBuf::from("/tmp/cache/react/18.2.0");

        state.extract_active.insert(key.clone());
        state.download_waiters.insert(key.clone(), vec![waiter]);

        state.handle_done(OpDone::Extract {
            key: key.clone(),
            result: Ok(cache_path.clone()),
        });

        assert!(!state.extract_active.contains(&key));
        assert_eq!(state.download_done[&key], cache_path);
        assert_eq!(state.clone_queue.len(), 1);
    }

    #[tokio::test]
    async fn queue_clone_dedupes_inflight_target() {
        let mut state = state();
        let target = PathBuf::from("/tmp/project/node_modules/react");
        let request = clone_request("react", "18.2.0", target.to_string_lossy().as_ref());
        let (first, _first_rx) = oneshot::channel();
        let (second, _second_rx) = oneshot::channel();

        state.queue_clone(request.clone(), Some(first));
        state.queue_clone(request, Some(second));

        assert_eq!(state.clone_waiters[&clone_key(&target)].len(), 2);
        assert_eq!(state.ops.len(), 1);
    }

    #[tokio::test]
    async fn prefetch_clone_waits_for_pending_parent() {
        let mut state = state();
        let parent = PathBuf::from("/tmp/project/node_modules/parent");
        let child = PathBuf::from("/tmp/project/node_modules/parent/node_modules/child");
        let parent_request = clone_request("parent", "1.0.0", parent.to_string_lossy().as_ref());
        let child_request = InstallCloneRequest {
            parent: Some(parent.clone()),
            ..clone_request("child", "1.0.0", child.to_string_lossy().as_ref())
        };

        state.queue_clone(parent_request, None);
        state.queue_clone(child_request, None);

        assert_eq!(state.blocked_by_parent[&clone_key(&parent)].len(), 1);
        assert_eq!(state.ops.len(), 1);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn queue_clone_normalizes_windows_path_separators() {
        let mut state = state();
        let forward = PathBuf::from("node_modules/@scope/pkg/node_modules/dep");
        let backward = PathBuf::from("node_modules\\@scope\\pkg\\node_modules\\dep");
        let first = clone_request("dep", "1.0.0", forward.to_string_lossy().as_ref());
        let second = clone_request("dep", "1.0.0", backward.to_string_lossy().as_ref());
        let key = clone_key(&forward);
        let (first_tx, _first_rx) = oneshot::channel();
        let (second_tx, _second_rx) = oneshot::channel();

        state.queue_clone(first, Some(first_tx));
        state.queue_clone(second, Some(second_tx));

        assert_eq!(state.clone_waiters[&key].len(), 2);
        assert_eq!(state.ops.len(), 1);
    }
}
