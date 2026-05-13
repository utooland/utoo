use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use futures::stream::{FuturesUnordered, StreamExt};
use tokio::sync::{mpsc, oneshot};

use crate::util::cloner::clone_package_from_cache;
use crate::util::downloader::{download_and_extract_to_cache, resolve_seeded_cache_path};
use crate::util::user_config::get_manifests_concurrency_limit_sync;

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
    },
    Download {
        key: String,
        result: Result<PathBuf, String>,
    },
    Clone {
        target: PathBuf,
        result: Result<(), String>,
    },
}

#[derive(Clone)]
pub struct InstallScheduler {
    tx: mpsc::UnboundedSender<Command>,
}

pub struct InstallSchedulerHandle {
    scheduler: InstallScheduler,
    handle: tokio::task::JoinHandle<()>,
}

impl InstallSchedulerHandle {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let handle = tokio::spawn(async move {
            SchedulerState::new(rx).run().await;
        });
        Self {
            scheduler: InstallScheduler { tx },
            handle,
        }
    }

    pub fn scheduler(&self) -> InstallScheduler {
        self.scheduler.clone()
    }

    pub async fn shutdown(self) {
        let _ = self.scheduler.tx.send(Command::Shutdown);
        if let Err(e) = self.handle.await {
            tracing::warn!("Install scheduler task failed: {e}");
        }
    }
}

impl InstallScheduler {
    #[cfg(test)]
    pub(crate) fn closed_for_test() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        drop(rx);
        Self { tx }
    }

    pub fn prefetch_download(&self, name: String, version: String, tarball_url: String) {
        let _ = self.tx.send(Command::PrefetchDownload(PackageFetch {
            name,
            version,
            tarball_url,
        }));
    }

    pub fn prefetch_clone(
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

    pub async fn ensure_clone(
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
    download_limit: usize,
    clone_limit: usize,
    download_done: HashMap<String, PathBuf>,
    download_active: HashSet<String>,
    download_waiters: HashMap<String, Vec<CloneSpec>>,
    download_queue: VecDeque<PackageFetch>,
    clone_done: HashSet<PathBuf>,
    clone_active: HashSet<PathBuf>,
    clone_waiters: HashMap<PathBuf, Vec<CloneResponder>>,
    blocked_by_parent: HashMap<PathBuf, Vec<CloneSpec>>,
    clone_queue: VecDeque<ReadyClone>,
    ops: FuturesUnordered<tokio::task::JoinHandle<OpDone>>,
}

impl SchedulerState {
    fn new(rx: mpsc::UnboundedReceiver<Command>) -> Self {
        Self {
            rx,
            shutdown: false,
            download_limit: get_manifests_concurrency_limit_sync().max(1),
            clone_limit: clone_concurrency_limit(),
            download_done: HashMap::new(),
            download_active: HashSet::new(),
            download_waiters: HashMap::new(),
            download_queue: VecDeque::new(),
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

        self.fail_pending("install scheduler stopped before work completed");
    }

    fn is_idle(&self) -> bool {
        self.download_queue.is_empty()
            && self.clone_queue.is_empty()
            && self.download_active.is_empty()
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
        self.ops.push(tokio::spawn(async move {
            let result = resolve_seeded_cache_path(
                &task_spec.package.name,
                &task_spec.package.version,
                &task_spec.package.tarball_url,
            )
            .await
            .map_err(|e| format!("{e:#}"));
            OpDone::SeededCache { spec, result }
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
        while self.download_active.len() < self.download_limit {
            let Some(package) = self.download_queue.pop_front() else {
                break;
            };
            let key = package.key();
            if self.download_done.contains_key(&key) || !self.download_active.insert(key.clone()) {
                continue;
            }

            self.ops.push(tokio::spawn(async move {
                let result = download_and_extract_to_cache(
                    &package.name,
                    &package.version,
                    &package.tarball_url,
                )
                .await
                .map_err(|e| format!("{e:#}"));
                OpDone::Download { key, result }
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
                OpDone::Clone { target, result }
            }));
        }
    }

    fn handle_done(&mut self, done: OpDone) {
        match done {
            OpDone::SeededCache { spec, result } => match result {
                Ok(Some(cache_path)) => self.clone_queue.push_back(ReadyClone { spec, cache_path }),
                Ok(None) => self.ensure_download(spec.package.clone(), Some(spec)),
                Err(error) => self.complete_clone(spec.target, Err(error)),
            },
            OpDone::Download { key, result } => {
                self.download_active.remove(&key);
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
            OpDone::Clone { target, result } => {
                self.clone_active.remove(&target);
                self.complete_clone(target, result);
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
        .map(|n| (n.get() * 4).clamp(4, 32))
        .unwrap_or(8)
}
