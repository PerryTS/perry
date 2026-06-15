//! Runtime-owned native async promise completion tokens.
//!
//! Native wrappers can hand an opaque token to worker threads and complete it
//! later. Completion requests are exactly-once, queue a main-thread settlement,
//! and expose every Promise/result/handle slot through a mutable GC scanner so
//! copied-minor evacuation can rewrite them.

use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::async_context::{capture_context, scan_snapshot_roots_mut, AsyncContextSnapshot};

use super::{set_promise_context_snapshot, Promise};

const STATE_PENDING: u8 = 0;
const STATE_QUEUED: u8 = 1;
const STATE_COMPLETED: u8 = 2;

const THREAD_ANY: u8 = 0;
const THREAD_MAIN: u8 = 1;

const DEFAULT_CANCEL_REASON: &str = "Native async operation cancelled";
const WRONG_THREAD_REASON: &str = "Native async completion requested from the wrong thread";

pub const PERRY_NATIVE_ASYNC_OK: i32 = 0;
pub const PERRY_NATIVE_ASYNC_ALREADY_COMPLETED: i32 = 1;
pub const PERRY_NATIVE_ASYNC_WRONG_THREAD: i32 = 2;
pub const PERRY_NATIVE_ASYNC_INVALID: i32 = 3;

pub const PERRY_NATIVE_ASYNC_THREAD_MAIN: u32 = 1 << 0;

pub const PERRY_NATIVE_ASYNC_CLEANUP_ON_REJECT: u32 = 1 << 0;
pub const PERRY_NATIVE_ASYNC_CLEANUP_ON_CANCEL: u32 = 1 << 1;
pub const PERRY_NATIVE_ASYNC_CLEANUP_ON_SUCCESS: u32 = 1 << 2;
const DEFAULT_CLEANUP_FLAGS: u32 =
    PERRY_NATIVE_ASYNC_CLEANUP_ON_REJECT | PERRY_NATIVE_ASYNC_CLEANUP_ON_CANCEL;

#[derive(Clone, Copy)]
struct AttachedHandle {
    value_bits: u64,
    cleanup_flags: u32,
}

enum PendingPayload {
    ResolveBits(u64),
    RejectBits(u64),
    RejectString(Vec<u8>),
    Cancel,
    WrongThread,
}

impl PendingPayload {
    fn resolve(bits: u64) -> Self {
        Self::ResolveBits(bits)
    }

    fn reject(bits: u64) -> Self {
        Self::RejectBits(bits)
    }

    fn reject_string(bytes: Vec<u8>) -> Self {
        Self::RejectString(bytes)
    }

    fn cancel() -> Self {
        Self::Cancel
    }

    fn wrong_thread() -> Self {
        Self::WrongThread
    }
}

struct TokenSlots {
    promise: usize,
    payload: Option<PendingPayload>,
    handles: Vec<AttachedHandle>,
    context: AsyncContextSnapshot,
}

/// Opaque native async completion token. The allocation is intentionally
/// leaked after completion so stale duplicate completion attempts can still
/// read the terminal state and return `ALREADY_COMPLETED` instead of turning
/// into a use-after-free.
#[repr(C)]
pub struct NativeAsyncCompletion {
    state: AtomicU8,
    thread_policy: u8,
    main_thread_id: u64,
    slots: Mutex<TokenSlots>,
}

unsafe impl Send for NativeAsyncCompletion {}
unsafe impl Sync for NativeAsyncCompletion {}

#[derive(Default)]
struct NativeAsyncRegistry {
    tokens: Vec<usize>,
    by_promise: HashMap<usize, usize>,
    pending: VecDeque<usize>,
}

static REGISTRY: OnceLock<Mutex<NativeAsyncRegistry>> = OnceLock::new();

fn registry() -> &'static Mutex<NativeAsyncRegistry> {
    REGISTRY.get_or_init(|| Mutex::new(NativeAsyncRegistry::default()))
}

fn current_thread_id() -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::thread::current().id().hash(&mut hasher);
    hasher.finish()
}

fn token_from_ptr<'a>(token: *mut NativeAsyncCompletion) -> Option<&'a NativeAsyncCompletion> {
    if token.is_null() {
        None
    } else {
        Some(unsafe { &*token })
    }
}

fn enqueue_payload(
    token_ptr: *mut NativeAsyncCompletion,
    payload: PendingPayload,
    return_status: i32,
) -> i32 {
    let Some(token) = token_from_ptr(token_ptr) else {
        return PERRY_NATIVE_ASYNC_INVALID;
    };
    if token
        .state
        .compare_exchange(
            STATE_PENDING,
            STATE_QUEUED,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        return PERRY_NATIVE_ASYNC_ALREADY_COMPLETED;
    }

    {
        let mut registry = crate::gc::lock_gc_root_registry(registry());
        let mut slots = token
            .slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        slots.payload = Some(payload);
        registry.pending.push_back(token_ptr as usize);
    }
    crate::event_pump::js_notify_main_thread();
    return_status
}

fn enqueue_with_thread_policy(
    token: *mut NativeAsyncCompletion,
    payload: PendingPayload,
    return_status: i32,
) -> i32 {
    let Some(native_token) = token_from_ptr(token) else {
        return PERRY_NATIVE_ASYNC_INVALID;
    };
    if native_token.thread_policy == THREAD_MAIN
        && current_thread_id() != native_token.main_thread_id
    {
        return enqueue_payload(
            token,
            PendingPayload::wrong_thread(),
            PERRY_NATIVE_ASYNC_WRONG_THREAD,
        );
    }
    enqueue_payload(token, payload, return_status)
}

fn complete_bits(token: *mut NativeAsyncCompletion, bits: u64, fulfilled: bool) -> i32 {
    let payload = if fulfilled {
        PendingPayload::resolve(bits)
    } else {
        PendingPayload::reject(bits)
    };
    enqueue_with_thread_policy(token, payload, PERRY_NATIVE_ASYNC_OK)
}

fn bytes_value_bits(bytes: &[u8]) -> u64 {
    let ptr = if bytes.is_empty() {
        crate::string::js_string_from_bytes(std::ptr::null(), 0)
    } else {
        crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
    };
    crate::value::JSValue::string_ptr(ptr).bits()
}

fn string_value_bits(message: &str) -> u64 {
    bytes_value_bits(message.as_bytes())
}

fn payload_to_settlement(payload: PendingPayload) -> (bool, u64, u32) {
    match payload {
        PendingPayload::ResolveBits(bits) => (true, bits, PERRY_NATIVE_ASYNC_CLEANUP_ON_SUCCESS),
        PendingPayload::RejectBits(bits) => (false, bits, PERRY_NATIVE_ASYNC_CLEANUP_ON_REJECT),
        PendingPayload::RejectString(bytes) => (
            false,
            bytes_value_bits(&bytes),
            PERRY_NATIVE_ASYNC_CLEANUP_ON_REJECT,
        ),
        PendingPayload::Cancel => (
            false,
            string_value_bits(DEFAULT_CANCEL_REASON),
            PERRY_NATIVE_ASYNC_CLEANUP_ON_CANCEL,
        ),
        PendingPayload::WrongThread => (
            false,
            string_value_bits(WRONG_THREAD_REASON),
            PERRY_NATIVE_ASYNC_CLEANUP_ON_REJECT,
        ),
    }
}

fn remove_token_from_registry(token_ptr: usize, promise: usize) {
    let mut registry = crate::gc::lock_gc_root_registry(registry());
    registry.tokens.retain(|&candidate| candidate != token_ptr);
    registry.pending.retain(|&candidate| candidate != token_ptr);
    if promise != 0 {
        registry.by_promise.remove(&promise);
    }
}

fn make_token_for_promise(
    promise: *mut Promise,
    flags: u32,
    register_by_promise: bool,
) -> *mut NativeAsyncCompletion {
    let thread_policy = if flags & PERRY_NATIVE_ASYNC_THREAD_MAIN != 0 {
        THREAD_MAIN
    } else {
        THREAD_ANY
    };
    let token = Box::new(NativeAsyncCompletion {
        state: AtomicU8::new(STATE_PENDING),
        thread_policy,
        main_thread_id: current_thread_id(),
        slots: Mutex::new(TokenSlots {
            promise: promise as usize,
            payload: None,
            handles: Vec::new(),
            context: capture_context(),
        }),
    });
    let token_ptr = Box::into_raw(token);
    {
        let mut registry = crate::gc::lock_gc_root_registry(registry());
        registry.tokens.push(token_ptr as usize);
        if register_by_promise && !promise.is_null() {
            registry
                .by_promise
                .insert(promise as usize, token_ptr as usize);
        }
    }
    let context = {
        let token = unsafe { &*token_ptr };
        token
            .slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .context
            .clone()
    };
    set_promise_context_snapshot(promise, context);
    token_ptr
}

fn adopt_or_get_token_for_promise(promise: *mut Promise) -> *mut NativeAsyncCompletion {
    if promise.is_null() {
        return std::ptr::null_mut();
    }
    let existing = {
        let registry = crate::gc::lock_gc_root_registry(registry());
        registry.by_promise.get(&(promise as usize)).copied()
    };
    if let Some(token) = existing {
        return token as *mut NativeAsyncCompletion;
    }
    make_token_for_promise(promise, 0, true)
}

/// Allocate a native async completion token and its JS-visible Promise.
#[no_mangle]
pub extern "C" fn js_native_async_completion_new(flags: u32) -> *mut NativeAsyncCompletion {
    let promise = super::js_promise_new();
    make_token_for_promise(promise, flags, true)
}

/// Return the JS-visible Promise owned by a native async completion token.
#[no_mangle]
pub extern "C" fn js_native_async_completion_promise(
    token: *mut NativeAsyncCompletion,
) -> *mut Promise {
    let Some(token) = token_from_ptr(token) else {
        return std::ptr::null_mut();
    };
    let slots = token
        .slots
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    slots.promise as *mut Promise
}

/// Resolve a native async token with already-encoded JSValue bits.
#[no_mangle]
pub extern "C" fn js_native_async_completion_resolve_bits(
    token: *mut NativeAsyncCompletion,
    bits: u64,
) -> i32 {
    complete_bits(token, bits, true)
}

/// Reject a native async token with already-encoded JSValue bits.
#[no_mangle]
pub extern "C" fn js_native_async_completion_reject_bits(
    token: *mut NativeAsyncCompletion,
    bits: u64,
) -> i32 {
    complete_bits(token, bits, false)
}

/// Reject a native async token with caller-owned UTF-8 bytes.
///
/// The bytes are copied before enqueueing so worker threads do not allocate
/// Perry runtime strings; string allocation happens while draining on the main
/// thread.
#[no_mangle]
pub extern "C" fn js_native_async_completion_reject_string(
    token: *mut NativeAsyncCompletion,
    data: *const u8,
    len: usize,
) -> i32 {
    if data.is_null() && len > 0 {
        return PERRY_NATIVE_ASYNC_INVALID;
    }
    if len > u32::MAX as usize {
        return PERRY_NATIVE_ASYNC_INVALID;
    }
    let bytes = if len == 0 {
        Vec::new()
    } else {
        // Copy before returning so caller-owned storage can be dropped or reused.
        unsafe { std::slice::from_raw_parts(data, len).to_vec() }
    };
    enqueue_with_thread_policy(
        token,
        PendingPayload::reject_string(bytes),
        PERRY_NATIVE_ASYNC_OK,
    )
}

/// Cancel a native async token, rejecting its Promise with the default reason.
#[no_mangle]
pub extern "C" fn js_native_async_completion_cancel(token: *mut NativeAsyncCompletion) -> i32 {
    enqueue_with_thread_policy(token, PendingPayload::cancel(), PERRY_NATIVE_ASYNC_OK)
}

/// Attach a JS native-handle value to a token for cleanup according to flags.
#[no_mangle]
pub extern "C" fn js_native_async_completion_attach_handle(
    token: *mut NativeAsyncCompletion,
    handle_bits: u64,
    cleanup_flags: u32,
) -> i32 {
    let Some(token) = token_from_ptr(token) else {
        return PERRY_NATIVE_ASYNC_INVALID;
    };
    if token.state.load(Ordering::Acquire) != STATE_PENDING {
        return PERRY_NATIVE_ASYNC_ALREADY_COMPLETED;
    }
    let flags = if cleanup_flags == 0 {
        DEFAULT_CLEANUP_FLAGS
    } else {
        cleanup_flags
    };
    let mut slots = token
        .slots
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if token.state.load(Ordering::Acquire) != STATE_PENDING {
        return PERRY_NATIVE_ASYNC_ALREADY_COMPLETED;
    }
    slots.handles.push(AttachedHandle {
        value_bits: handle_bits,
        cleanup_flags: flags,
    });
    PERRY_NATIVE_ASYNC_OK
}

/// Legacy shim: resolve the token associated with a Promise pointer.
#[no_mangle]
pub extern "C" fn js_native_async_completion_resolve_promise_bits(
    promise: *mut Promise,
    bits: u64,
) -> i32 {
    let token = adopt_or_get_token_for_promise(promise);
    js_native_async_completion_resolve_bits(token, bits)
}

/// Legacy shim: reject the token associated with a Promise pointer.
#[no_mangle]
pub extern "C" fn js_native_async_completion_reject_promise_bits(
    promise: *mut Promise,
    bits: u64,
) -> i32 {
    let token = adopt_or_get_token_for_promise(promise);
    js_native_async_completion_reject_bits(token, bits)
}

/// Drain queued native async completions on the main thread.
#[no_mangle]
pub extern "C" fn js_native_async_process_pending() -> i32 {
    let pending: Vec<usize> = {
        let mut registry = crate::gc::lock_gc_root_registry(registry());
        registry.pending.drain(..).collect()
    };
    let mut processed = 0i32;
    for token_ptr in pending {
        let token = unsafe { &*(token_ptr as *const NativeAsyncCompletion) };
        let (promise, payload, handles, context) = {
            let mut slots = token
                .slots
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let promise = slots.promise;
            let payload = slots.payload.take();
            let handles = std::mem::take(&mut slots.handles);
            let context = std::mem::take(&mut slots.context);
            slots.promise = 0;
            (promise, payload, handles, context)
        };
        let Some(payload) = payload else {
            token.state.store(STATE_COMPLETED, Ordering::Release);
            remove_token_from_registry(token_ptr, promise);
            continue;
        };

        let scope = crate::gc::RuntimeHandleScope::new();
        let promise_handle = scope.root_raw_mut_ptr(promise as *mut Promise);
        let handle_roots: Vec<_> = handles
            .iter()
            .map(|handle| scope.root_nanbox_u64(handle.value_bits))
            .collect();
        let context_roots = crate::async_context::root_snapshot(&scope, &context);
        let (fulfilled, result_bits, cleanup_mask) = payload_to_settlement(payload);
        let result_handle = scope.root_nanbox_u64(result_bits);
        let promise_ptr = promise_handle.get_raw_mut_ptr::<Promise>();
        if !promise_ptr.is_null() {
            let mut context = context;
            crate::async_context::refresh_snapshot_from_roots(&mut context, &context_roots);
            set_promise_context_snapshot(promise_ptr, context);
            for (idx, handle) in handles.iter().enumerate() {
                if handle.cleanup_flags & cleanup_mask != 0 {
                    let bits = handle_roots[idx].get_nanbox_u64();
                    crate::native_handle::js_native_handle_dispose(f64::from_bits(bits));
                }
            }
            if fulfilled {
                super::js_promise_resolve(
                    promise_ptr,
                    f64::from_bits(result_handle.get_nanbox_u64()),
                );
            } else {
                super::js_promise_reject(
                    promise_ptr,
                    f64::from_bits(result_handle.get_nanbox_u64()),
                );
            }
            processed += 1;
        }
        token.state.store(STATE_COMPLETED, Ordering::Release);
        remove_token_from_registry(token_ptr, promise);
    }
    processed
}

/// Drop the native-async completion token associated with a Promise that was
/// settled *synchronously* (via `js_promise_resolve`/`js_promise_reject`)
/// outside the deferred completion machinery.
///
/// Ext crates such as `perry-ext-events` allocate their `events.once` Promise
/// through perry-ffi's `JsPromise::new()` → `perry_ffi_promise_new()`, which
/// registers a native-async token (and pins the Promise) so a worker can
/// resolve it later. `events.once`, however, settles synchronously from
/// `emit(...)` and deliberately bypasses the deferred resolve path (see the
/// extern comment in perry-ext-events). That bypass never runs
/// `js_native_async_process_pending`, so the token stays in the registry
/// forever and `js_native_async_has_active()` keeps reporting work — the
/// process hangs after the awaited event already fired (the
/// `events.once(emitter, name)` + `emit` hang). Calling this right after the
/// synchronous settle removes the orphaned token (mirroring the cleanup
/// `js_native_async_process_pending` performs) so the event loop can drain.
#[no_mangle]
pub extern "C" fn js_native_async_drop_promise_token(promise: *mut Promise) {
    if promise.is_null() {
        return;
    }
    let token_ptr = {
        let registry = crate::gc::lock_gc_root_registry(registry());
        registry.by_promise.get(&(promise as usize)).copied()
    };
    if let Some(token_ptr) = token_ptr {
        let token = unsafe { &*(token_ptr as *const NativeAsyncCompletion) };
        token.state.store(STATE_COMPLETED, Ordering::Release);
        remove_token_from_registry(token_ptr, promise as usize);
    }
}

/// Return 1 while there are live or queued native async completions.
#[no_mangle]
pub extern "C" fn js_native_async_has_active() -> i32 {
    let registry = crate::gc::lock_gc_root_registry(registry());
    if registry.tokens.is_empty() && registry.pending.is_empty() {
        0
    } else {
        1
    }
}

/// Mutable GC scanner for live native async token slots.
pub fn scan_native_async_completion_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut registry = crate::gc::lock_gc_root_registry(registry());
    let mut moved_promises = Vec::new();
    for &token_ptr in &registry.tokens {
        let token = unsafe { &*(token_ptr as *const NativeAsyncCompletion) };
        let mut slots = token
            .slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let old_promise = slots.promise;
        visitor.visit_usize_slot(&mut slots.promise);
        if old_promise != 0 && old_promise != slots.promise {
            moved_promises.push((old_promise, slots.promise, token_ptr));
        }
        if let Some(PendingPayload::ResolveBits(bits) | PendingPayload::RejectBits(bits)) =
            &mut slots.payload
        {
            visitor.visit_nanbox_u64_slot(bits);
        }
        for handle in &mut slots.handles {
            visitor.visit_nanbox_u64_slot(&mut handle.value_bits);
        }
        scan_snapshot_roots_mut(&mut slots.context, visitor);
    }
    for (old_promise, new_promise, token_ptr) in moved_promises {
        registry.by_promise.remove(&old_promise);
        if new_promise != 0 {
            registry.by_promise.insert(new_promise, token_ptr);
        }
    }
}

#[cfg(test)]
static TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
pub(crate) fn test_native_async_lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
pub(crate) fn test_reset_native_async_registry() {
    let mut registry = crate::gc::lock_gc_root_registry(registry());
    registry.tokens.clear();
    registry.by_promise.clear();
    registry.pending.clear();
}

#[cfg(test)]
pub(crate) fn test_native_async_slot_snapshot(
    token: *mut NativeAsyncCompletion,
) -> (usize, Option<u64>, Vec<u64>) {
    let token = unsafe { &*token };
    let slots = token
        .slots
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    (
        slots.promise,
        slots.payload.as_ref().and_then(|payload| match payload {
            PendingPayload::ResolveBits(bits) | PendingPayload::RejectBits(bits) => Some(*bits),
            PendingPayload::RejectString(_)
            | PendingPayload::Cancel
            | PendingPayload::WrongThread => None,
        }),
        slots
            .handles
            .iter()
            .map(|handle| handle.value_bits)
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    static CLEANUP_FINALIZER_CALLS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn count_cleanup_finalizer(_resource: *mut c_void, _hint: *mut c_void) {
        CLEANUP_FINALIZER_CALLS.fetch_add(1, AtomicOrdering::SeqCst);
    }

    fn native_handle_type_id(name: &str) -> i64 {
        crate::native_handle::js_native_handle_type_id(name.as_ptr(), name.len())
    }

    fn owned_native_handle(name: &str, resource: i64) -> f64 {
        crate::native_handle::js_native_handle_new_owned(
            resource,
            native_handle_type_id(name),
            0,
            0,
            count_cleanup_finalizer as *mut c_void,
            name.as_ptr(),
            name.len() as i64,
        )
    }

    unsafe fn assert_heap_string_value(value: f64, expected: &[u8]) {
        let value = crate::value::JSValue::from_bits(value.to_bits());
        assert!(value.is_string(), "expected heap string JSValue");
        let ptr = value.as_string_ptr();
        assert!(!ptr.is_null(), "expected non-null string pointer");
        assert_eq!((*ptr).byte_len as usize, expected.len());
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let bytes = std::slice::from_raw_parts(data, expected.len());
        assert_eq!(bytes, expected);
    }

    #[test]
    fn resolves_once_and_duplicate_returns_status() {
        let _guard = test_native_async_lock();
        test_reset_native_async_registry();
        let token = js_native_async_completion_new(0);
        let promise = js_native_async_completion_promise(token);
        assert_eq!(
            js_native_async_completion_resolve_bits(token, 7.0f64.to_bits()),
            0
        );
        assert_eq!(
            js_native_async_completion_reject_bits(token, 9.0f64.to_bits()),
            PERRY_NATIVE_ASYNC_ALREADY_COMPLETED
        );
        assert_eq!(js_native_async_process_pending(), 1);
        assert_eq!(super::super::js_promise_state(promise), 1);
        assert_eq!(super::super::js_promise_value(promise), 7.0);
        assert_eq!(
            js_native_async_completion_resolve_bits(token, 10.0f64.to_bits()),
            PERRY_NATIVE_ASYNC_ALREADY_COMPLETED
        );
    }

    #[test]
    fn cancel_rejects_with_default_reason() {
        let _guard = test_native_async_lock();
        test_reset_native_async_registry();
        let token = js_native_async_completion_new(0);
        let promise = js_native_async_completion_promise(token);
        assert_eq!(
            js_native_async_completion_cancel(token),
            PERRY_NATIVE_ASYNC_OK
        );
        assert_eq!(js_native_async_process_pending(), 1);
        assert_eq!(super::super::js_promise_state(promise), 2);
        assert_ne!(super::super::js_promise_reason(promise).to_bits(), 0);
    }

    #[test]
    fn reject_string_copies_bytes_before_main_thread_settlement() {
        let _guard = test_native_async_lock();
        test_reset_native_async_registry();

        let token = js_native_async_completion_new(0);
        let promise = js_native_async_completion_promise(token);
        assert_eq!(
            js_native_async_completion_reject_string(token, std::ptr::null(), 1),
            PERRY_NATIVE_ASYNC_INVALID
        );

        let expected = b"native async copied rejection".to_vec();
        let mut message = String::from_utf8(expected.clone()).expect("valid utf-8");
        let data = message.as_ptr();
        let len = message.len();
        assert_eq!(
            js_native_async_completion_reject_string(token, data, len),
            PERRY_NATIVE_ASYNC_OK
        );
        message.clear();
        message.push_str("mutated after queue");
        drop(message);

        assert_eq!(js_native_async_process_pending(), 1);
        assert_eq!(super::super::js_promise_state(promise), 2);
        unsafe {
            assert_heap_string_value(super::super::js_promise_reason(promise), &expected);
        }
    }

    #[test]
    fn cancel_cleanup_disposes_attached_handle_once() {
        let _guard = test_native_async_lock();
        test_reset_native_async_registry();
        CLEANUP_FINALIZER_CALLS.store(0, AtomicOrdering::SeqCst);

        let token = js_native_async_completion_new(0);
        let promise = js_native_async_completion_promise(token);
        let handle = owned_native_handle("NativeAsyncCancel", 0x3456);
        assert_eq!(
            js_native_async_completion_attach_handle(
                token,
                handle.to_bits(),
                PERRY_NATIVE_ASYNC_CLEANUP_ON_CANCEL,
            ),
            PERRY_NATIVE_ASYNC_OK
        );
        assert_eq!(
            js_native_async_completion_cancel(token),
            PERRY_NATIVE_ASYNC_OK
        );
        assert_eq!(js_native_async_process_pending(), 1);

        assert_eq!(super::super::js_promise_state(promise), 2);
        unsafe {
            assert_heap_string_value(
                super::super::js_promise_reason(promise),
                DEFAULT_CANCEL_REASON.as_bytes(),
            );
        }
        assert_eq!(CLEANUP_FINALIZER_CALLS.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(crate::native_handle::js_native_handle_dispose(handle), 0);
        assert_eq!(CLEANUP_FINALIZER_CALLS.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn success_cleanup_disposes_attached_handle_once() {
        let _guard = test_native_async_lock();
        test_reset_native_async_registry();
        CLEANUP_FINALIZER_CALLS.store(0, AtomicOrdering::SeqCst);

        let token = js_native_async_completion_new(0);
        let promise = js_native_async_completion_promise(token);
        let handle = owned_native_handle("NativeAsyncSuccessCleanup", 0x6789);
        assert_eq!(
            js_native_async_completion_attach_handle(
                token,
                handle.to_bits(),
                PERRY_NATIVE_ASYNC_CLEANUP_ON_SUCCESS,
            ),
            PERRY_NATIVE_ASYNC_OK
        );
        assert_eq!(
            js_native_async_completion_resolve_bits(token, 6.0f64.to_bits()),
            PERRY_NATIVE_ASYNC_OK
        );
        assert_eq!(js_native_async_process_pending(), 1);

        assert_eq!(super::super::js_promise_state(promise), 1);
        assert_eq!(super::super::js_promise_value(promise), 6.0);
        assert_eq!(CLEANUP_FINALIZER_CALLS.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(crate::native_handle::js_native_handle_dispose(handle), 0);
        assert_eq!(CLEANUP_FINALIZER_CALLS.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn main_thread_token_wrong_thread_rejects() {
        let _guard = test_native_async_lock();
        test_reset_native_async_registry();
        let token = js_native_async_completion_new(PERRY_NATIVE_ASYNC_THREAD_MAIN);
        let promise = js_native_async_completion_promise(token);
        let token_addr = token as usize;
        let status = std::thread::spawn(move || {
            js_native_async_completion_resolve_bits(
                token_addr as *mut NativeAsyncCompletion,
                1.0f64.to_bits(),
            )
        })
        .join()
        .expect("thread join");
        assert_eq!(status, PERRY_NATIVE_ASYNC_WRONG_THREAD);
        assert_eq!(js_native_async_process_pending(), 1);
        assert_eq!(super::super::js_promise_state(promise), 2);
    }

    #[test]
    fn main_thread_token_reject_string_wrong_thread_uses_wrong_thread_reason() {
        let _guard = test_native_async_lock();
        test_reset_native_async_registry();
        let token = js_native_async_completion_new(PERRY_NATIVE_ASYNC_THREAD_MAIN);
        let promise = js_native_async_completion_promise(token);
        let token_addr = token as usize;
        let status = std::thread::spawn(move || {
            let message = String::from("worker string rejection");
            js_native_async_completion_reject_string(
                token_addr as *mut NativeAsyncCompletion,
                message.as_ptr(),
                message.len(),
            )
        })
        .join()
        .expect("thread join");

        assert_eq!(status, PERRY_NATIVE_ASYNC_WRONG_THREAD);
        assert_eq!(js_native_async_process_pending(), 1);
        assert_eq!(super::super::js_promise_state(promise), 2);
        unsafe {
            assert_heap_string_value(
                super::super::js_promise_reason(promise),
                WRONG_THREAD_REASON.as_bytes(),
            );
        }
    }

    #[test]
    fn main_thread_token_wrong_thread_cancel_rejects_instead_of_cancelling() {
        let _guard = test_native_async_lock();
        test_reset_native_async_registry();
        CLEANUP_FINALIZER_CALLS.store(0, AtomicOrdering::SeqCst);

        let token = js_native_async_completion_new(PERRY_NATIVE_ASYNC_THREAD_MAIN);
        let promise = js_native_async_completion_promise(token);
        let cancel_only_handle = owned_native_handle("NativeAsyncCancelWrongThread", 0x2468);
        assert_eq!(
            js_native_async_completion_attach_handle(
                token,
                cancel_only_handle.to_bits(),
                PERRY_NATIVE_ASYNC_CLEANUP_ON_CANCEL,
            ),
            PERRY_NATIVE_ASYNC_OK
        );

        let token_addr = token as usize;
        let status = std::thread::spawn(move || {
            js_native_async_completion_cancel(token_addr as *mut NativeAsyncCompletion)
        })
        .join()
        .expect("thread join");

        assert_eq!(status, PERRY_NATIVE_ASYNC_WRONG_THREAD);
        assert_eq!(js_native_async_process_pending(), 1);
        assert_eq!(super::super::js_promise_state(promise), 2);
        unsafe {
            assert_heap_string_value(
                super::super::js_promise_reason(promise),
                WRONG_THREAD_REASON.as_bytes(),
            );
        }
        assert_eq!(CLEANUP_FINALIZER_CALLS.load(AtomicOrdering::SeqCst), 0);
        assert_eq!(
            crate::native_handle::js_native_handle_dispose(cancel_only_handle),
            1
        );
        assert_eq!(CLEANUP_FINALIZER_CALLS.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn reject_cleanup_disposes_attached_handles_but_success_keeps_them_live() {
        let _guard = test_native_async_lock();
        test_reset_native_async_registry();
        CLEANUP_FINALIZER_CALLS.store(0, AtomicOrdering::SeqCst);

        let reject_token = js_native_async_completion_new(0);
        let reject_handle = owned_native_handle("NativeAsyncReject", 0x1234);
        assert_eq!(
            js_native_async_completion_attach_handle(
                reject_token,
                reject_handle.to_bits(),
                PERRY_NATIVE_ASYNC_CLEANUP_ON_REJECT,
            ),
            PERRY_NATIVE_ASYNC_OK
        );
        assert_eq!(
            js_native_async_completion_reject_bits(reject_token, 2.0f64.to_bits()),
            PERRY_NATIVE_ASYNC_OK
        );
        assert_eq!(js_native_async_process_pending(), 1);
        assert_eq!(CLEANUP_FINALIZER_CALLS.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(
            crate::native_handle::js_native_handle_dispose(reject_handle),
            0
        );

        let success_token = js_native_async_completion_new(0);
        let success_handle = owned_native_handle("NativeAsyncSuccess", 0x5678);
        assert_eq!(
            js_native_async_completion_attach_handle(
                success_token,
                success_handle.to_bits(),
                PERRY_NATIVE_ASYNC_CLEANUP_ON_REJECT,
            ),
            PERRY_NATIVE_ASYNC_OK
        );
        assert_eq!(
            js_native_async_completion_resolve_bits(success_token, 4.0f64.to_bits()),
            PERRY_NATIVE_ASYNC_OK
        );
        assert_eq!(js_native_async_process_pending(), 1);
        assert_eq!(CLEANUP_FINALIZER_CALLS.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(
            crate::native_handle::js_native_handle_dispose(success_handle),
            1
        );
        assert_eq!(CLEANUP_FINALIZER_CALLS.load(AtomicOrdering::SeqCst), 2);
    }
}
