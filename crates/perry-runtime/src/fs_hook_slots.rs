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
    UTF8_STREAM_DESTROY_VALUE.set(crate::fs::utf8_stream_destroy_value);
    UTF8_STREAM_EMIT_VALUE.set(crate::fs::utf8_stream_emit_value);
    UTF8_STREAM_END_VALUE.set(crate::fs::utf8_stream_end_value);
    UTF8_STREAM_LISTENER_COUNT_VALUE.set(crate::fs::utf8_stream_listener_count_value);
    UTF8_STREAM_OFF_VALUE.set(crate::fs::utf8_stream_off_value);
    UTF8_STREAM_ON_VALUE.set(crate::fs::utf8_stream_on_value);
    UTF8_STREAM_REMOVE_ALL_VALUE.set(crate::fs::utf8_stream_remove_all_value);
    UTF8_STREAM_WRITE_VALUE.set(crate::fs::utf8_stream_write_value);
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

static UTF8_STREAM_DESTROY_VALUE: Hook<fn(f64) -> Option<f64>> = Hook::empty();
pub(crate) fn utf8_stream_destroy_value(stream_value: f64) -> Option<f64> {
    UTF8_STREAM_DESTROY_VALUE
        .get()
        .and_then(|probe| probe(stream_value))
}

static UTF8_STREAM_EMIT_VALUE: Hook<fn(f64, f64, f64) -> Option<f64>> = Hook::empty();
pub(crate) fn utf8_stream_emit_value(stream_value: f64, event: f64, arg: f64) -> Option<f64> {
    UTF8_STREAM_EMIT_VALUE
        .get()
        .and_then(|probe| probe(stream_value, event, arg))
}

static UTF8_STREAM_END_VALUE: Hook<fn(f64, f64) -> Option<f64>> = Hook::empty();
pub(crate) fn utf8_stream_end_value(stream_value: f64, chunk: f64) -> Option<f64> {
    UTF8_STREAM_END_VALUE
        .get()
        .and_then(|probe| probe(stream_value, chunk))
}

static UTF8_STREAM_LISTENER_COUNT_VALUE: Hook<fn(f64, f64) -> Option<f64>> = Hook::empty();
pub(crate) fn utf8_stream_listener_count_value(stream_value: f64, event: f64) -> Option<f64> {
    UTF8_STREAM_LISTENER_COUNT_VALUE
        .get()
        .and_then(|probe| probe(stream_value, event))
}

static UTF8_STREAM_OFF_VALUE: Hook<fn(f64, f64, f64) -> Option<f64>> = Hook::empty();
pub(crate) fn utf8_stream_off_value(stream_value: f64, event: f64, cb: f64) -> Option<f64> {
    UTF8_STREAM_OFF_VALUE
        .get()
        .and_then(|probe| probe(stream_value, event, cb))
}

static UTF8_STREAM_ON_VALUE: Hook<fn(f64, f64, f64, bool) -> Option<f64>> = Hook::empty();
pub(crate) fn utf8_stream_on_value(
    stream_value: f64,
    event: f64,
    cb: f64,
    once: bool,
) -> Option<f64> {
    UTF8_STREAM_ON_VALUE
        .get()
        .and_then(|probe| probe(stream_value, event, cb, once))
}

static UTF8_STREAM_REMOVE_ALL_VALUE: Hook<fn(f64, f64) -> Option<f64>> = Hook::empty();
pub(crate) fn utf8_stream_remove_all_value(stream_value: f64, event: f64) -> Option<f64> {
    UTF8_STREAM_REMOVE_ALL_VALUE
        .get()
        .and_then(|probe| probe(stream_value, event))
}

static UTF8_STREAM_WRITE_VALUE: Hook<fn(f64, f64) -> Option<f64>> = Hook::empty();
pub(crate) fn utf8_stream_write_value(stream_value: f64, data: f64) -> Option<f64> {
    UTF8_STREAM_WRITE_VALUE
        .get()
        .and_then(|probe| probe(stream_value, data))
}
