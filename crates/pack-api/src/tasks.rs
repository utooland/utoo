use std::{sync::Arc, time::Duration};

use anyhow::Result;
use either::Either;
use turbo_tasks::{
    TaskId, TurboTasks, TurboTasksApi, UpdateInfo, Vc, task_statistics::TaskStatisticsApi,
    trace::TraceRawVcs,
};

use turbo_tasks_backend::{NoopBackingStorage, TurboTasksBackend};

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
use turbo_tasks_backend::DefaultBackingStorage;

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub type UtooTurboTasks =
    Arc<TurboTasks<TurboTasksBackend<Either<DefaultBackingStorage, NoopBackingStorage>>>>;

// In WASM builds there is no disk persistence; always use the noop backing storage.
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub type UtooTurboTasks = Arc<TurboTasks<TurboTasksBackend<NoopBackingStorage>>>;

#[derive(Clone)]
pub enum BundlerTurboTasks {
    Memory(Arc<TurboTasks<TurboTasksBackend<NoopBackingStorage>>>),
    #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
    PersistentCaching(Arc<TurboTasks<TurboTasksBackend<DefaultBackingStorage>>>),
}

impl BundlerTurboTasks {
    pub fn dispose_root_task(&self, task: TaskId) {
        match self {
            BundlerTurboTasks::Memory(turbo_tasks) => turbo_tasks.dispose_root_task(task),
            #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
            BundlerTurboTasks::PersistentCaching(turbo_tasks) => {
                turbo_tasks.dispose_root_task(task)
            }
        }
    }

    pub fn spawn_root_task<T, F, Fut>(&self, functor: F) -> TaskId
    where
        T: Send,
        F: Fn() -> Fut + Send + Sync + Clone + 'static,
        Fut: Future<Output = Result<Vc<T>>> + Send,
    {
        match self {
            BundlerTurboTasks::Memory(turbo_tasks) => turbo_tasks.spawn_root_task(functor),
            #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
            BundlerTurboTasks::PersistentCaching(turbo_tasks) => {
                turbo_tasks.spawn_root_task(functor)
            }
        }
    }

    pub async fn run_once<T: TraceRawVcs + Send + 'static>(
        &self,
        future: impl Future<Output = Result<T>> + Send + 'static,
    ) -> Result<T> {
        match self {
            BundlerTurboTasks::Memory(turbo_tasks) => turbo_tasks.run_once(future).await,
            #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
            BundlerTurboTasks::PersistentCaching(turbo_tasks) => turbo_tasks.run_once(future).await,
        }
    }

    pub async fn aggregated_update_info(
        &self,
        aggregation: Duration,
        timeout: Duration,
    ) -> Option<UpdateInfo> {
        match self {
            BundlerTurboTasks::Memory(turbo_tasks) => {
                turbo_tasks
                    .aggregated_update_info(aggregation, timeout)
                    .await
            }
            #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
            BundlerTurboTasks::PersistentCaching(turbo_tasks) => {
                turbo_tasks
                    .aggregated_update_info(aggregation, timeout)
                    .await
            }
        }
    }

    pub async fn get_or_wait_aggregated_update_info(&self, aggregation: Duration) -> UpdateInfo {
        match self {
            BundlerTurboTasks::Memory(turbo_tasks) => {
                turbo_tasks
                    .get_or_wait_aggregated_update_info(aggregation)
                    .await
            }
            #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
            BundlerTurboTasks::PersistentCaching(turbo_tasks) => {
                turbo_tasks
                    .get_or_wait_aggregated_update_info(aggregation)
                    .await
            }
        }
    }

    pub async fn stop_and_wait(&self) {
        match self {
            BundlerTurboTasks::Memory(turbo_tasks) => turbo_tasks.stop_and_wait().await,
            #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
            BundlerTurboTasks::PersistentCaching(turbo_tasks) => turbo_tasks.stop_and_wait().await,
        }
    }

    pub fn task_statistics(&self) -> &TaskStatisticsApi {
        match self {
            BundlerTurboTasks::Memory(turbo_tasks) => turbo_tasks.task_statistics(),
            #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
            BundlerTurboTasks::PersistentCaching(turbo_tasks) => turbo_tasks.task_statistics(),
        }
    }
}
