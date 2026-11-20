//! Foreign Node-API buffers enqueue callbacks through their owning host.
use crate::feature_hooks::Hook;
use crate::node_api_host::FinalizerRecord;
static ENQUEUE: Hook<fn(FinalizerRecord)> = Hook::empty();
pub(crate) fn enqueue(finalizer: FinalizerRecord) {
    if let Some(run) = ENQUEUE.get() {
        run(finalizer)
    }
}
pub(crate) fn install() {
    crate::node_api_host::install_storage_resolvers();
    ENQUEUE.set(crate::node_api_host::enqueue_finalizer);
}

type TlsResolver = fn(u32) -> Result<*mut u8, crate::tls_hot::AccessError>;
static ENV_STORAGE: Hook<TlsResolver> = Hook::empty();
pub(crate) fn set_env_storage(resolve: TlsResolver) {
    ENV_STORAGE.set(resolve)
}
pub(crate) fn resolve_env_storage(idx: u32) -> Result<*mut u8, crate::tls_hot::AccessError> {
    ENV_STORAGE
        .get()
        .expect("Node API installer must precede its TLS storage")(idx)
}
static LEGACY_STORAGE: Hook<TlsResolver> = Hook::empty();
pub(crate) fn set_legacy_storage(resolve: TlsResolver) {
    LEGACY_STORAGE.set(resolve)
}
pub(crate) fn resolve_legacy_storage(idx: u32) -> Result<*mut u8, crate::tls_hot::AccessError> {
    LEGACY_STORAGE
        .get()
        .expect("Node API installer must precede its TLS storage")(idx)
}
static FINALIZER_STORAGE: Hook<TlsResolver> = Hook::empty();
pub(crate) fn set_finalizer_storage(resolve: TlsResolver) {
    FINALIZER_STORAGE.set(resolve)
}
pub(crate) fn resolve_finalizer_storage(idx: u32) -> Result<*mut u8, crate::tls_hot::AccessError> {
    FINALIZER_STORAGE
        .get()
        .expect("Node API installer must precede its TLS storage")(idx)
}

type CompletionQueue = crate::node_api_host::AsyncCompletionQueue;
static NEW_COMPLETION_QUEUE: Hook<fn() -> CompletionQueue> = Hook::empty();
pub(crate) fn set_completion_queue(make: fn() -> CompletionQueue) {
    NEW_COMPLETION_QUEUE.set(make)
}
pub(crate) fn new_completion_queue() -> CompletionQueue {
    NEW_COMPLETION_QUEUE
        .get()
        .expect("Node API installer must precede its completion queue")()
}
