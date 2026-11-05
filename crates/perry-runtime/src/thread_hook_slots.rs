//! Completion and clone operations selected by the thread/clone installer.
use crate::feature_hooks::Hook;
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
/// FileHandle codec supplied by fs before the first handle can exist.
/// This existing descriptor can be filled without linking the thread serializer.
pub(crate) struct FsThreadCodec {
    /// `is_fs_filehandle_value(v) || filehandle_object_fd(v).is_some()`.
    pub is_filehandle: fn(f64) -> bool,
    /// `build_detached_filehandle_object` (FileHandle shape, `fd === -1`).
    pub build_detached: fn() -> f64,
}

static FS_THREAD_CODEC: AtomicPtr<FsThreadCodec> = AtomicPtr::new(ptr::null_mut());

pub(crate) fn arm_fs_thread_codec(codec: &'static FsThreadCodec) {
    // `black_box` for the same reason as `NM_INSTALL_ALL_HOOK`: a
    // single-store AtomicPtr gets speculatively devirtualized by
    // whole-program optimization (only one value is ever stored, so the
    // compiler proves it and re-materializes the direct reference —
    // re-pinning everything this hook exists to unpin).
    FS_THREAD_CODEC.store(
        std::hint::black_box(codec as *const FsThreadCodec as *mut FsThreadCodec),
        Ordering::Release,
    );
}

pub(crate) fn fs_thread_codec() -> Option<&'static FsThreadCodec> {
    let p = FS_THREAD_CODEC.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        // SAFETY: only ever stores `&'static FsThreadCodec`.
        Some(unsafe { &*p })
    }
}

static PROCESS_PENDING: Hook<extern "C" fn() -> i32> = Hook::empty();
static HAS_PENDING: Hook<extern "C" fn() -> i32> = Hook::empty();
static PURGE: Hook<fn(crate::agent::AgentId)> = Hook::empty();
static CLONE: Hook<fn(f64, &[usize]) -> f64> = Hook::empty();
static DIAG_PROCESS: Hook<fn() -> i32> = Hook::empty();
static DIAG_PENDING: Hook<fn() -> bool> = Hook::empty();
pub(crate) fn diagnostics_channel_process_pending() -> i32 {
    DIAG_PROCESS.get().map_or(0, |run| run())
}
pub(crate) fn diagnostics_channel_has_pending_publishes() -> bool {
    DIAG_PENDING.get().is_some_and(|run| run())
}
pub(crate) fn js_thread_process_pending() -> i32 {
    PROCESS_PENDING.get().map_or(0, |run| run())
}
pub(crate) fn js_thread_has_pending() -> i32 {
    HAS_PENDING.get().map_or(0, |run| run())
}
pub(crate) fn purge_agent_thread_results(agent: crate::agent::AgentId) {
    if let Some(run) = PURGE.get() {
        run(agent);
    }
}
pub(crate) fn clone_with_transfers(value: f64, transfer: &[usize]) -> f64 {
    CLONE.get().map_or_else(
        || f64::from_bits(crate::value::JSValue::undefined().bits()),
        |run| run(value, transfer),
    )
}
pub(crate) fn install() {
    crate::perry_install_once!({
        ENQUEUE_DIAGNOSTIC.set(crate::node_submodules::diagnostics::enqueue_cross_thread_publish);
        DIAG_PROCESS.set(crate::node_submodules::diagnostics_channel_process_pending);
        DIAG_PENDING.set(crate::node_submodules::diagnostics_channel_has_pending_publishes);
        PROCESS_PENDING.set(crate::thread::js_thread_process_pending);
        HAS_PENDING.set(crate::thread::js_thread_has_pending);
        PURGE.set(crate::thread::purge_agent_thread_results);
        CLONE.set(crate::builtins::clone_with_transfers);
    });
}

static ENQUEUE_DIAGNOSTIC: Hook<
    fn(crate::node_submodules::diagnostics::DiagChannelKey, f64, bool),
> = Hook::empty();
pub(crate) fn enqueue_cross_thread_publish(
    key: crate::node_submodules::diagnostics::DiagChannelKey,
    data: f64,
    local_delivered: bool,
) {
    if let Some(publish) = ENQUEUE_DIAGNOSTIC.get() {
        publish(key, data, local_delivered);
    }
}
