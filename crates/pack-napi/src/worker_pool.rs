use napi::{Status, bindgen_prelude::Unknown, threadsafe_function::ThreadsafeFunction};
use napi_derive::napi;
use turbopack_node::worker_pool::{NapiTaskMessage, NapiWorkerCreation, NapiWorkerTermination};

type FatalThreadsafeFunction<T> = ThreadsafeFunction<
    T,
    Unknown<'static>,
    T,
    Status,
    /* CalleeHandled */ false,
    /* Weak */ true,
>;

#[napi]
pub fn register_worker_scheduler(
    #[napi(ts_arg_type = "(arg: NapiWorkerCreation) => any")] creator: FatalThreadsafeFunction<
        NapiWorkerCreation,
    >,
    #[napi(ts_arg_type = "(arg: NapiWorkerTermination) => any")]
    terminator: FatalThreadsafeFunction<NapiWorkerTermination>,
) -> napi::Result<()> {
    turbopack_node::worker_pool::register_worker_scheduler(creator, terminator)
}

#[napi]
pub fn worker_created(worker_id: u32) {
    turbopack_node::worker_pool::worker_created(worker_id);
}

#[napi]
pub async fn recv_task_message_in_worker(worker_id: u32) -> napi::Result<NapiTaskMessage> {
    turbopack_node::worker_pool::recv_task_message_in_worker(worker_id).await
}

#[napi]
pub fn send_task_message(message: NapiTaskMessage) -> napi::Result<()> {
    turbopack_node::worker_pool::send_task_message(message)
}
