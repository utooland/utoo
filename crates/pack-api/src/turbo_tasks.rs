use std::sync::Arc;

use turbo_tasks::TurboTasks;

use turbo_tasks_backend::TurboTasksBackend;

pub type UtooTurboTasks = Arc<TurboTasks<TurboTasksBackend>>;
