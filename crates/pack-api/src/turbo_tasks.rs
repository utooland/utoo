use std::sync::Arc;

use either::Either;
use turbo_tasks::TurboTasks;

use turbo_tasks_backend::{NoopBackingStorage, TurboBackingStorage, TurboTasksBackend};

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub type UtooTurboTasks =
    Arc<TurboTasks<TurboTasksBackend<Either<TurboBackingStorage, NoopBackingStorage>>>>;

// In WASM builds there is no disk persistence; always use the noop backing storage.
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub type UtooTurboTasks = Arc<TurboTasks<TurboTasksBackend<NoopBackingStorage>>>;
