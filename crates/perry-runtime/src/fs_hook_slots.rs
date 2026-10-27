//! Installer-owned access to filesystem state from generic runtime paths.
use crate::feature_hooks::Hook;
static CLONE_FD: Hook<fn(i32) -> Option<std::fs::File>> = Hook::empty();
static IS_STREAM: Hook<fn(f64, &str) -> bool> = Hook::empty();
pub(crate) fn try_clone_registered_fd(fd: i32) -> Option<std::fs::File> {
    CLONE_FD.get().and_then(|clone| clone(fd))
}
pub(crate) fn is_fs_stream_instance_value(value: f64, name: &str) -> bool {
    IS_STREAM.get().is_some_and(|check| check(value, name))
}
pub(crate) fn install() {
    CLONE_FD.set(crate::fs::try_clone_registered_fd);
    IS_STREAM.set(crate::fs::is_fs_stream_instance_value);
}

type TlsResolver = fn(u32) -> Result<*mut u8, crate::tls_hot::AccessError>;
static WATCH_QUEUE_RESOLVER: Hook<TlsResolver> = Hook::empty();
static WATCH_SHARED_RESOLVER: Hook<TlsResolver> = Hook::empty();
pub(crate) fn resolve_watch_queue(idx: u32) -> Result<*mut u8, crate::tls_hot::AccessError> {
    WATCH_QUEUE_RESOLVER
        .get()
        .expect("filesystem installer must precede watcher storage")(idx)
}
pub(crate) fn resolve_watch_shared(idx: u32) -> Result<*mut u8, crate::tls_hot::AccessError> {
    WATCH_SHARED_RESOLVER
        .get()
        .expect("filesystem installer must precede watcher storage")(idx)
}
pub(crate) fn install_watch_resolvers(queue: TlsResolver, shared: TlsResolver) {
    WATCH_QUEUE_RESOLVER.set(queue);
    WATCH_SHARED_RESOLVER.set(shared);
}
