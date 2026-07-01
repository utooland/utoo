use napi::{
    Env,
    threadsafe_function::{ErrorStrategy, ThreadsafeFunction},
};
use napi_derive::napi;
use turbopack_node::worker_pool::{NapiTaskMessage, NapiWorkerCreation, NapiWorkerTermination};

#[napi]
pub fn register_worker_scheduler(
    env: Env,
    creator: ThreadsafeFunction<NapiWorkerCreation, ErrorStrategy::Fatal>,
    terminator: ThreadsafeFunction<NapiWorkerTermination, ErrorStrategy::Fatal>,
) -> napi::Result<()> {
    turbopack_node::worker_pool::register_worker_scheduler(env, creator, terminator)
}

#[napi]
pub fn worker_created(worker_id: u32) {
    turbopack_node::worker_pool::worker_created(worker_id);
}

#[napi]
pub async fn recv_task_message_in_worker(worker_id: u32) -> napi::Result<NapiTaskMessage> {
    turbopack_node::worker_pool::recv_task_message_in_worker(worker_id).await
}
