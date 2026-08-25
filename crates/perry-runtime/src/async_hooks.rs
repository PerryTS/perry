//! Native async_hooks lifecycle support.
//!
//! This module owns the process-wide hook list, async resource ids, and the
//! thread-local execution/trigger id stack used by the compiled runtime.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::ptr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex};

use crate::array::{js_array_length, ArrayHeader};
use crate::closure::{
    js_closure_alloc, js_closure_call1, js_closure_call4, js_closure_call_array,
    js_closure_get_capture_f64, js_closure_get_capture_ptr, js_closure_set_capture_f64,
    js_closure_set_capture_ptr, js_register_closure_rest, ClosureHeader,
};
use crate::object::{js_object_get_field_by_name, ObjectHeader};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::{JSValue, POINTER_MASK};

mod provider_ffi;
pub use provider_ffi::{
    defer_destroy_after_check_turns, js_async_hooks_provider_defer_destroy,
    js_async_hooks_provider_destroy, js_async_hooks_provider_enter, js_async_hooks_provider_init,
    js_async_hooks_provider_init_with_trigger, js_async_hooks_provider_leave,
    js_async_hooks_provider_run_catching, js_async_hooks_provider_run_catching_deferred_destroy,
    js_async_hooks_provider_run_catching_deferred_destroy_on_error,
    js_async_hooks_provider_run_catching_with_this,
};

const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
const TAG_UNDEFINED_F64: f64 = f64::from_bits(crate::value::TAG_UNDEFINED);

// Async ids start at 2: Node reserves id 1 for the bootstrap/root execution
// context, so the first user-visible resource (e.g. the first `setTimeout`)
// gets an id > 1 — observable through `executionAsyncId()` inside its
// callback (#789).
//
// #7680: `NEXT_ASYNC_ID`, `HOOKS_ACTIVE`, and `PROMISE_HOOKS_ACTIVE` (with `HOOKS` / `RESOURCES` /
// `GC_DESTROY_QUEUE` / `NEXT_CONTEXT_SNAPSHOT_ID` / `CONTEXT_SNAPSHOTS` /
// `ASYNC_WRAP_PROVIDERS` below) are `per_test_global!`: `reset_for_tests()`
// clears all nine from whatever thread runs it, and before this fix that
// thread could be any of four disjoint lock domains (this module's own
// private `TEST_LOCK`, `AsyncHookRuntimeTestGuard`'s private
// `ASYNC_HOOK_RUNTIME_TEST_LOCK`, the GC guards' shared lock via
// `CopyingNurseryTestGuard`, or — `gc/tests/alloc.rs`'s
// `test_async_hooks_promise_alloc_remains_malloc_tracked` — no lock at all).
// `resource_ids_are_monotonic_even_without_hooks`'s `b.async_id == a.async_id
// + 1` is exactly the #7672 shape: a neighbour's concurrent `init_resource`
// call turns that into `+ 2` and reads as "ids are not monotonic" rather than
// "a neighbour allocated one". Per-thread storage removes the need for any of
// the four locks, the same way #7674 did for the GC guards' own clear list —
// this module's `reset_for_tests()` is simply outside that list, so #7674's
// gate never saw it.
per_test_global! {
    static NEXT_ASYNC_ID: AtomicU64 = AtomicU64::new(2);
    pub static HOOKS_ACTIVE: AtomicUsize = AtomicUsize::new(0);
    static PROMISE_HOOKS_ACTIVE: AtomicUsize = AtomicUsize::new(0);
    static TOP_LEVEL_RESOURCE: AtomicU64 = AtomicU64::new(0);
    #[cfg(test)]
    static TEST_FORCE_RESOLVE_GC: AtomicUsize = AtomicUsize::new(0);
}

#[derive(Clone, Copy)]
pub struct AsyncResourceIds {
    pub async_id: u64,
    pub trigger_async_id: u64,
}

#[derive(Clone)]
struct ResourceMeta {
    // #854: async_hooks resource metadata; real createHook lifecycle is #789
    #[allow(dead_code)]
    type_name: String,
    // #854: async_hooks resource metadata; real createHook lifecycle is #789
    #[allow(dead_code)]
    trigger_async_id: u64,
    resource: f64,
    context: crate::async_context::AsyncContextSnapshot,
    destroyed: bool,
}

#[derive(Clone, Copy)]
struct HookCallbacks {
    init: *const ClosureHeader,
    before: *const ClosureHeader,
    after: *const ClosureHeader,
    destroy: *const ClosureHeader,
    promise_resolve: *const ClosureHeader,
}

#[derive(Clone, Copy)]
enum HookPhase {
    Init,
    Before,
    After,
    Destroy,
    PromiseResolve,
}

unsafe impl Send for HookCallbacks {}
unsafe impl Sync for HookCallbacks {}

impl HookCallbacks {
    fn empty() -> Self {
        Self {
            init: ptr::null(),
            before: ptr::null(),
            after: ptr::null(),
            destroy: ptr::null(),
            promise_resolve: ptr::null(),
        }
    }

    fn has_any(&self) -> bool {
        !self.init.is_null()
            || !self.before.is_null()
            || !self.after.is_null()
            || !self.destroy.is_null()
            || !self.promise_resolve.is_null()
    }

    fn for_phase(&self, phase: HookPhase) -> *const ClosureHeader {
        match phase {
            HookPhase::Init => self.init,
            HookPhase::Before => self.before,
            HookPhase::After => self.after,
            HookPhase::Destroy => self.destroy,
            HookPhase::PromiseResolve => self.promise_resolve,
        }
    }
}

struct HookRecord {
    callbacks: HookCallbacks,
    enabled: bool,
    track_promises: bool,
}

// #7680: see the `NEXT_ASYNC_ID` / `HOOKS_ACTIVE` comment above — these six
// are the rest of what `reset_for_tests()` clears, converted for the same
// reason. `scan_async_hooks_roots_mut` (below) already reaches `HOOKS`,
// `RESOURCES`, `CONTEXT_SNAPSHOTS` and `ASYNC_WRAP_PROVIDERS` through a
// registered scanner defined in THIS file, so `scripts/gc_runtime_root_holders.py`
// already covers them; `per_test_global!` only changes which instance a test
// thread resolves to, not the scanner's reach.
per_test_global! {
    static HOOKS: LazyLock<Mutex<Vec<HookRecord>>> = LazyLock::new(|| Mutex::new(Vec::new()));
    static RESOURCES: LazyLock<Mutex<HashMap<u64, ResourceMeta>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    static GC_DESTROY_QUEUE: LazyLock<Mutex<VecDeque<u64>>> =
        LazyLock::new(|| Mutex::new(VecDeque::new()));
    static NEXT_CONTEXT_SNAPSHOT_ID: AtomicUsize = AtomicUsize::new(1);
    static CONTEXT_SNAPSHOTS: LazyLock<
        Mutex<HashMap<usize, crate::async_context::AsyncContextSnapshot>>,
    > = LazyLock::new(|| Mutex::new(HashMap::new()));
    static ASYNC_WRAP_PROVIDERS: AtomicU64 = AtomicU64::new(0);
}

/// Live `AsyncResource` handles. Handles are raw `Box::into_raw` pointers
/// (never freed → membership is monotonic), NaN-boxed with POINTER_TAG like
/// heap objects — so the dynamic method path needs this registry to recognize
/// one BEFORE dereferencing it as an ObjectHeader (#789, mirrors the
/// BOX_REGISTRY pattern from #4898).
static ASYNC_RESOURCE_HANDLES: LazyLock<Mutex<HashSet<i64>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));
static ASYNC_RESOURCE_HANDLE_COUNT: AtomicUsize = AtomicUsize::new(0);
const ASYNC_RESOURCE_SUBCLASS_KEY: &[u8] = b"__perryAsyncResourceBacking";

/// Live `AsyncHook` handles, for the same dynamic-receiver reason as
/// `ASYNC_RESOURCE_HANDLES`. A helper that returns
/// `createHook(options).enable()` erases the static `AsyncHook` class before
/// its caller later invokes `disable()`.
static ASYNC_HOOK_HANDLES: LazyLock<Mutex<HashSet<i64>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));
static ASYNC_HOOK_HANDLE_COUNT: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static EXECUTION_STACK: RefCell<Vec<(u64, u64)>> = const { RefCell::new(Vec::new()) };
    static CURRENT_EXECUTION_ID: Cell<u64> = const { Cell::new(0) };
    static CURRENT_TRIGGER_ID: Cell<u64> = const { Cell::new(0) };
    // Node defers hook-list mutations made by a hook callback until the
    // outermost hook-delivery cascade has finished.  In particular, an init
    // callback can synchronously create another resource; that nested init
    // must still see the hook set that was active at the start of the outer
    // init.  Keep the last requested state for each hook while any lifecycle
    // callback is on the stack, then commit the batch at depth zero.
    static HOOK_CALLBACK_DEPTH: Cell<usize> = const { Cell::new(0) };
    static PENDING_HOOK_STATES: RefCell<HashMap<usize, bool>> = RefCell::new(HashMap::new());
}

pub struct AsyncHookHandle {
    index: usize,
}

pub struct AsyncResourceHandle {
    ids: AsyncResourceIds,
    event_emitter: i64,
}

pub(crate) fn is_async_resource_handle(handle: i64) -> bool {
    handle != 0 && ASYNC_RESOURCE_HANDLES.lock().unwrap().contains(&handle)
}

/// Resolve either a native `AsyncResource` handle or the ordinary object used
/// for a source-compiled subclass to its native backing allocation.
pub(crate) fn resolve_async_resource_handle(receiver: i64) -> Option<i64> {
    if is_async_resource_handle(receiver) {
        return Some(receiver);
    }
    let raw = receiver as usize;
    if !crate::value::addr_class::is_plausible_heap_addr(raw) {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_raw_mut_ptr(raw as *mut ObjectHeader);
    #[cfg(test)]
    if TEST_FORCE_RESOLVE_GC.swap(0, Ordering::Relaxed) != 0 {
        let _ = crate::gc::gc_collect_minor();
    }
    let key = js_string_from_bytes(
        ASYNC_RESOURCE_SUBCLASS_KEY.as_ptr(),
        ASYNC_RESOURCE_SUBCLASS_KEY.len() as u32,
    );
    let value = receiver
        .with_mut_ptr::<ObjectHeader, _>(|receiver| js_object_get_field_by_name(receiver, key));
    if !value.is_pointer() {
        return None;
    }
    let backing = value.as_pointer::<u8>() as i64;
    is_async_resource_handle(backing).then_some(backing)
}

#[cfg(test)]
pub(crate) fn test_force_next_async_resource_resolve_gc() {
    TEST_FORCE_RESOLVE_GC.store(1, Ordering::Relaxed);
}

#[cfg(test)]
pub(crate) fn test_link_async_resource_subclass(receiver: *mut ObjectHeader, backing: i64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_raw_mut_ptr(receiver);
    let key = js_string_from_bytes(
        ASYNC_RESOURCE_SUBCLASS_KEY.as_ptr(),
        ASYNC_RESOURCE_SUBCLASS_KEY.len() as u32,
    );
    receiver.with_mut_ptr::<ObjectHeader, _>(|receiver| {
        crate::object::js_object_set_field_by_name(
            receiver,
            key,
            crate::value::js_nanbox_pointer(backing),
        );
    });
}

#[inline(always)]
pub fn hooks_active() -> bool {
    HOOKS_ACTIVE.load(Ordering::Relaxed) != 0
}

/// Whether any enabled `AsyncHook` opted into Promise lifecycle tracking.
#[inline(always)]
pub fn promise_hooks_active() -> bool {
    PROMISE_HOOKS_ACTIVE.load(Ordering::Relaxed) != 0
}

#[inline]
pub fn execution_async_id_u64() -> u64 {
    CURRENT_EXECUTION_ID.with(Cell::get)
}

#[inline]
pub fn trigger_async_id_u64() -> u64 {
    CURRENT_TRIGGER_ID.with(Cell::get)
}

#[no_mangle]
pub extern "C" fn js_async_hooks_execution_async_id() -> f64 {
    execution_async_id_u64() as f64
}

#[no_mangle]
pub extern "C" fn js_async_hooks_trigger_async_id() -> f64 {
    async_id_to_js_number(trigger_async_id_u64())
}

#[no_mangle]
pub extern "C" fn js_async_hooks_execution_async_resource() -> f64 {
    let current_id = execution_async_id_u64();
    if current_id != 0 {
        if let Some(resource) = RESOURCES
            .lock()
            .unwrap()
            .get(&current_id)
            .map(|meta| meta.resource)
        {
            if !JSValue::from_bits(resource.to_bits()).is_undefined() {
                return resource;
            }
        }
    }

    let cached = TOP_LEVEL_RESOURCE.load(Ordering::Acquire);
    if cached != 0 {
        return f64::from_bits(cached);
    }

    // Node exposes one stable bootstrap resource for the top-level execution
    // scope. Returning a fresh object here made restoration checks fail after
    // every nested AsyncResource scope and also broke metadata inheritance in
    // init hooks.
    let obj = crate::object::js_object_alloc(0, 0);
    let value = crate::value::js_nanbox_pointer(obj as i64);
    TOP_LEVEL_RESOURCE.store(value.to_bits(), Ordering::Release);
    crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    value
}

const ASYNC_WRAP_PROVIDER_CONSTANTS: &[(&str, f64)] = &[
    ("NONE", 0.0),
    ("DIRHANDLE", 1.0),
    ("DNSCHANNEL", 2.0),
    ("ELDHISTOGRAM", 3.0),
    ("FILEHANDLE", 4.0),
    ("FILEHANDLECLOSEREQ", 5.0),
    ("BLOBREADER", 6.0),
    ("FSEVENTWRAP", 7.0),
    ("FSREQCALLBACK", 8.0),
    ("FSREQPROMISE", 9.0),
    ("GETADDRINFOREQWRAP", 10.0),
    ("GETNAMEINFOREQWRAP", 11.0),
    ("HEAPSNAPSHOT", 12.0),
    ("HTTP2SESSION", 13.0),
    ("HTTP2STREAM", 14.0),
    ("HTTP2PING", 15.0),
    ("HTTP2SETTINGS", 16.0),
    ("HTTPINCOMINGMESSAGE", 17.0),
    ("HTTPCLIENTREQUEST", 18.0),
    ("LOCKS", 19.0),
    ("JSSTREAM", 20.0),
    ("JSUDPWRAP", 21.0),
    ("MESSAGEPORT", 22.0),
    ("PIPECONNECTWRAP", 23.0),
    ("PIPESERVERWRAP", 24.0),
    ("PIPEWRAP", 25.0),
    ("PROCESSWRAP", 26.0),
    ("PROMISE", 27.0),
    ("QUERYWRAP", 28.0),
    ("QUIC_ENDPOINT", 29.0),
    ("QUIC_LOGSTREAM", 30.0),
    ("QUIC_PACKET", 31.0),
    ("QUIC_SESSION", 32.0),
    ("QUIC_STREAM", 33.0),
    ("QUIC_UDP", 34.0),
    ("SHUTDOWNWRAP", 35.0),
    ("SIGNALWRAP", 36.0),
    ("STATWATCHER", 37.0),
    ("STREAMPIPE", 38.0),
    ("TCPCONNECTWRAP", 39.0),
    ("TCPSERVERWRAP", 40.0),
    ("TCPWRAP", 41.0),
    ("TTYWRAP", 42.0),
    ("UDPSENDWRAP", 43.0),
    ("UDPWRAP", 44.0),
    ("SIGINTWATCHDOG", 45.0),
    ("WORKER", 46.0),
    ("WORKERCPUPROFILE", 47.0),
    ("WORKERCPUUSAGE", 48.0),
    ("WORKERHEAPPROFILE", 49.0),
    ("WORKERHEAPSNAPSHOT", 50.0),
    ("WORKERHEAPSTATISTICS", 51.0),
    ("WRITEWRAP", 52.0),
    ("ZLIB", 53.0),
    ("CHECKPRIMEREQUEST", 54.0),
    ("PBKDF2REQUEST", 55.0),
    ("KEYPAIRGENREQUEST", 56.0),
    ("KEYGENREQUEST", 57.0),
    ("KEYEXPORTREQUEST", 58.0),
    ("ARGON2REQUEST", 59.0),
    ("CIPHERREQUEST", 60.0),
    ("DERIVEBITSREQUEST", 61.0),
    ("HASHREQUEST", 62.0),
    ("RANDOMBYTESREQUEST", 63.0),
    ("RANDOMPRIMEREQUEST", 64.0),
    ("SCRYPTREQUEST", 65.0),
    ("SIGNREQUEST", 66.0),
    ("TLSWRAP", 67.0),
    ("VERIFYREQUEST", 68.0),
];

pub fn js_async_hooks_async_wrap_providers() -> f64 {
    let cached = ASYNC_WRAP_PROVIDERS.load(Ordering::Acquire);
    if cached != 0 {
        return f64::from_bits(cached);
    }

    let obj =
        crate::object::js_object_alloc_null_proto(0, ASYNC_WRAP_PROVIDER_CONSTANTS.len() as u32);
    for (name, value) in ASYNC_WRAP_PROVIDER_CONSTANTS {
        let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, key, *value);
    }
    let value = crate::value::js_nanbox_pointer(obj as i64);
    let value = crate::object::js_object_freeze(value);
    ASYNC_WRAP_PROVIDERS.store(value.to_bits(), Ordering::Release);
    crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    value
}

// #854: pointer-boxing helper retained for async_hooks resource tracking (#789)
#[allow(dead_code)]
#[inline]
fn box_ptr(ptr: *const u8) -> f64 {
    f64::from_bits(POINTER_TAG | (ptr as u64 & POINTER_MASK))
}

/// NaN-box a `StringHeader` pointer with `STRING_TAG` so JS sees a real
/// string (#789): the `init` hook's `type` argument is a string like
/// `"PROMISE"` — boxing it as a generic `POINTER_TAG` made the callback
/// observe `[object Object]` instead.
#[inline]
fn box_string(ptr: *const u8) -> f64 {
    f64::from_bits(STRING_TAG | (ptr as u64 & POINTER_MASK))
}

fn ptr_from_nanboxed(value: f64) -> *const u8 {
    let bits = value.to_bits();
    let tag = bits & TAG_MASK;
    if tag != POINTER_TAG && tag != STRING_TAG {
        return ptr::null();
    }
    (bits & POINTER_MASK) as *const u8
}

fn closure_from_value(value: f64) -> *const ClosureHeader {
    ptr_from_nanboxed(value) as *const ClosureHeader
}

fn object_field(obj_value: f64, name: &[u8]) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_nanbox_f64(obj_value);
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32) as *const StringHeader;
    let key_handle = scope.root_string_ptr(key);
    let obj = ptr_from_nanboxed(obj_handle.get_nanbox_f64()) as *const ObjectHeader;
    if obj.is_null() {
        return TAG_UNDEFINED_F64;
    }
    f64::from_bits(js_object_get_field_by_name(obj, key_handle.get_raw_const_ptr()).bits())
}

/// #3089 — `createHook(options)` destructures `options` immediately, so a
/// nullish top-level value throws a plain `TypeError` (no error code) with
/// Node's "Cannot destructure property 'init' of …" message *before* any
/// callback is read. Non-nullish primitives (e.g. `0`) are accepted because
/// destructuring them simply yields no callback fields.
fn validate_create_hook_options(options: f64) {
    let jv = JSValue::from_bits(options.to_bits());
    let received = if jv.is_undefined() {
        "'undefined' as it is undefined"
    } else if jv.is_null() {
        "'object null' as it is null"
    } else {
        return;
    };
    let message = format!("Cannot destructure property 'init' of {}.", received);
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
}

/// #3089 — a *present* (non-`undefined`) hook member must be callable, matching
/// Node's `validateFunction(value, 'hook.<name>')` which throws
/// `TypeError [ERR_ASYNC_CALLBACK]` "hook.<name> must be a function". A missing
/// or `undefined` member is allowed (left as a null callback).
fn validate_hook_member(value: f64, member: &str) -> *const ClosureHeader {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return ptr::null();
    }
    if is_callable_value(value) {
        return closure_from_value(value);
    }
    let message = format!("hook.{} must be a function", member);
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_ASYNC_CALLBACK")
}

fn callbacks_from_options(options: f64) -> (HookCallbacks, bool) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let options_handle = scope.root_nanbox_f64(options);
    let mut callbacks = HookCallbacks::empty();
    let init = scope.root_nanbox_f64(object_field(options_handle.get_nanbox_f64(), b"init"));
    let before = scope.root_nanbox_f64(object_field(options_handle.get_nanbox_f64(), b"before"));
    let after = scope.root_nanbox_f64(object_field(options_handle.get_nanbox_f64(), b"after"));
    let destroy = scope.root_nanbox_f64(object_field(options_handle.get_nanbox_f64(), b"destroy"));
    let promise_resolve = scope.root_nanbox_f64(object_field(
        options_handle.get_nanbox_f64(),
        b"promiseResolve",
    ));
    // Node reads `trackPromises` after the five callback properties. Missing
    // means true; a present value must be a boolean.
    let track_promises = scope.root_nanbox_f64(object_field(
        options_handle.get_nanbox_f64(),
        b"trackPromises",
    ));
    callbacks.init = validate_hook_member(init.get_nanbox_f64(), "init");
    callbacks.before = validate_hook_member(before.get_nanbox_f64(), "before");
    callbacks.after = validate_hook_member(after.get_nanbox_f64(), "after");
    callbacks.destroy = validate_hook_member(destroy.get_nanbox_f64(), "destroy");
    callbacks.promise_resolve =
        validate_hook_member(promise_resolve.get_nanbox_f64(), "promiseResolve");
    let track_promises_value = track_promises.get_nanbox_f64();
    let track_promises_kind = JSValue::from_bits(track_promises_value.to_bits());
    let track_promises = if track_promises_kind.is_undefined() {
        true
    } else if track_promises_kind.is_bool() {
        track_promises_kind.as_bool()
    } else {
        let message = format!(
            "The \"trackPromises\" argument must be of type boolean. Received {}",
            crate::fs::validate::describe_received(track_promises_value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
    };
    if !track_promises && !callbacks.promise_resolve.is_null() {
        crate::fs::validate::throw_type_error_with_code(
            "The argument 'trackPromises' must not be false when promiseResolve is enabled. Received false",
            "ERR_INVALID_ARG_VALUE",
        );
    }
    (callbacks, track_promises)
}

#[no_mangle]
pub extern "C" fn js_async_hooks_create_hook(options: f64) -> i64 {
    validate_create_hook_options(options);
    let (callbacks, track_promises) = callbacks_from_options(options);
    let mut hooks = HOOKS.lock().unwrap();
    let index = hooks.len();
    hooks.push(HookRecord {
        callbacks,
        enabled: false,
        track_promises,
    });
    let handle = Box::into_raw(Box::new(AsyncHookHandle { index })) as i64;
    ASYNC_HOOK_HANDLES.lock().unwrap().insert(handle);
    ASYNC_HOOK_HANDLE_COUNT.fetch_add(1, Ordering::Relaxed);
    handle
}

/// Dynamic method dispatch for `AsyncHook` values whose static class was lost
/// through a helper return or `any`-typed binding.
pub fn try_async_hook_method_dispatch(handle: i64, method_name: &str) -> Option<f64> {
    if ASYNC_HOOK_HANDLE_COUNT.load(Ordering::Relaxed) == 0
        || !matches!(method_name, "enable" | "disable")
        || !ASYNC_HOOK_HANDLES.lock().unwrap().contains(&handle)
    {
        return None;
    }
    let result = match method_name {
        "enable" => js_async_hook_enable(handle),
        "disable" => js_async_hook_disable(handle),
        _ => unreachable!("gated above"),
    };
    Some(crate::value::js_nanbox_pointer(result))
}

#[no_mangle]
pub extern "C" fn js_async_hook_enable(handle: i64) -> i64 {
    if handle == 0 {
        return handle;
    }
    let hook = unsafe { &*(handle as *const AsyncHookHandle) };
    if HOOK_CALLBACK_DEPTH.with(Cell::get) != 0 {
        PENDING_HOOK_STATES.with(|pending| {
            pending.borrow_mut().insert(hook.index, true);
        });
        return handle;
    }
    set_hook_enabled(hook.index, true);
    handle
}

fn set_hook_enabled(index: usize, enabled: bool) {
    let mut hooks = HOOKS.lock().unwrap();
    if let Some(record) = hooks.get_mut(index) {
        if record.enabled == enabled {
            return;
        }
        let delta_is_visible = record.callbacks.has_any();
        if delta_is_visible {
            if enabled {
                HOOKS_ACTIVE.fetch_add(1, Ordering::Relaxed);
                if record.track_promises {
                    PROMISE_HOOKS_ACTIVE.fetch_add(1, Ordering::Relaxed);
                }
            } else {
                HOOKS_ACTIVE.fetch_sub(1, Ordering::Relaxed);
                if record.track_promises {
                    PROMISE_HOOKS_ACTIVE.fetch_sub(1, Ordering::Relaxed);
                }
            }
        }
        record.enabled = enabled;
    }
}

#[no_mangle]
pub extern "C" fn js_async_hook_disable(handle: i64) -> i64 {
    if handle == 0 {
        return handle;
    }
    let hook = unsafe { &*(handle as *const AsyncHookHandle) };
    if HOOK_CALLBACK_DEPTH.with(Cell::get) != 0 {
        PENDING_HOOK_STATES.with(|pending| {
            pending.borrow_mut().insert(hook.index, false);
        });
        return handle;
    }
    set_hook_enabled(hook.index, false);
    handle
}

fn enabled_callbacks(is_promise: bool) -> Vec<HookCallbacks> {
    if !hooks_active() {
        return Vec::new();
    }
    HOOKS
        .lock()
        .unwrap()
        .iter()
        .filter(|hook| hook.enabled && (!is_promise || hook.track_promises))
        .map(|hook| hook.callbacks)
        .collect()
}

fn with_hook_callbacks(
    phase: HookPhase,
    is_promise: bool,
    mut f: impl FnMut(*const ClosureHeader),
) {
    if !hooks_active() {
        return;
    }
    let callbacks = enabled_callbacks(is_promise);
    HOOK_CALLBACK_DEPTH.with(|depth| depth.set(depth.get() + 1));

    // Hook membership is snapshotted once per lifecycle phase: disabling a
    // hook from another hook callback must not remove it from the phase already
    // in progress, and enabling one must not add it. Re-entrant lifecycle
    // delivery is nevertheless required: an async operation started by an
    // init/destroy callback is a new phase with a fresh membership snapshot.
    // A process-wide "inside a hook" guard used to suppress those nested
    // phases entirely.
    //
    // The callback pointers in this snapshot still have to remain moving-GC
    // roots. A callback can allocate arbitrary JS objects; without the handles
    // the first hook could evacuate the remaining hooks while their copied raw
    // pointers stayed stale in this Rust Vec.
    let scope = crate::gc::RuntimeHandleScope::new();
    let rooted: Vec<_> = callbacks
        .iter()
        .map(|callbacks| scope.root_raw_const_ptr(callbacks.for_phase(phase)))
        .collect();
    let mut thrown = None;
    for callback in rooted {
        let outcome = callback.with_const_ptr::<ClosureHeader, _>(|callback| {
            if !callback.is_null() {
                return crate::exception::js_call_catching(|| {
                    f(callback);
                    f64::from_bits(crate::value::TAG_UNDEFINED)
                });
            }
            Ok(f64::from_bits(crate::value::TAG_UNDEFINED))
        });
        if let Err(error) = outcome {
            thrown = Some(scope.root_nanbox_f64(error));
            break;
        }
    }
    let outermost = HOOK_CALLBACK_DEPTH.with(|depth| {
        let next = depth.get().saturating_sub(1);
        depth.set(next);
        next == 0
    });
    if outermost {
        let pending = PENDING_HOOK_STATES.with(|states| std::mem::take(&mut *states.borrow_mut()));
        for (index, enabled) in pending {
            set_hook_enabled(index, enabled);
        }
    }
    if let Some(error) = thrown {
        crate::exception::js_throw(error.get_nanbox_f64());
    }
}

/// Model the Promise that Node uses to evaluate an ESM entry module. Perry's
/// compiled entry does not allocate that wrapper Promise, but its init event
/// is observable by hooks enabled during module evaluation.
pub(crate) fn init_esm_evaluation_promise() {
    if !promise_hooks_active() {
        return;
    }
    let resource = crate::object::js_object_alloc_null_proto(0, 0);
    let value = crate::value::js_nanbox_pointer(resource as i64);
    let _ = init_resource("PROMISE", value, false);
}

/// Reserve an async id for a resource whose lifecycle is not observable yet.
///
/// Promises created before the first hook is enabled still need a stable id so
/// a later child reaction can name that promise as its trigger. They must not
/// be inserted into `RESOURCES`, because doing so would turn the resource value
/// into a strong GC root before any observer exists.
pub fn reserve_resource_ids(trigger_async_id: u64) -> AsyncResourceIds {
    AsyncResourceIds {
        async_id: NEXT_ASYNC_ID.fetch_add(1, Ordering::Relaxed),
        trigger_async_id,
    }
}

pub fn init_resource(type_name: &str, resource: f64, force_allocate: bool) -> AsyncResourceIds {
    init_resource_with_trigger(
        type_name,
        resource,
        force_allocate,
        execution_async_id_u64(),
    )
}

pub fn init_resource_with_trigger(
    type_name: &str,
    resource: f64,
    force_allocate: bool,
    trigger_async_id: u64,
) -> AsyncResourceIds {
    if !force_allocate && !hooks_active() {
        return AsyncResourceIds {
            async_id: 0,
            trigger_async_id,
        };
    }

    let async_id = NEXT_ASYNC_ID.fetch_add(1, Ordering::Relaxed);
    // Native resources such as an accepted TCP socket can be materialized by
    // the main-thread pump after the originating callback has returned.  At
    // that point Perry's current execution id is the bootstrap scope even
    // though the provider has an explicit trigger.  Inherit the trigger's
    // captured store in that narrow case so AsyncLocalStorage crosses the
    // native hand-off just as it does in Node.
    let context = if execution_async_id_u64() == 0 && trigger_async_id != 0 {
        RESOURCES
            .lock()
            .unwrap()
            .get(&trigger_async_id)
            .map(|meta| meta.context.clone())
            .unwrap_or_else(crate::async_context::capture_context)
    } else {
        crate::async_context::capture_context()
    };
    RESOURCES.lock().unwrap().insert(
        async_id,
        ResourceMeta {
            type_name: type_name.to_string(),
            trigger_async_id,
            resource,
            context,
            destroyed: false,
        },
    );

    emit_init(async_id, type_name, trigger_async_id, resource);
    AsyncResourceIds {
        async_id,
        trigger_async_id,
    }
}

fn emit_init(async_id: u64, type_name: &str, trigger_async_id: u64, resource: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let resource_handle = scope.root_nanbox_f64(resource);
    let type_ptr = js_string_from_bytes(type_name.as_ptr(), type_name.len() as u32);
    let type_value_handle = scope.root_nanbox_f64(box_string(type_ptr as *const u8));
    with_hook_callbacks(HookPhase::Init, type_name == "PROMISE", |callback| {
        js_closure_call4(
            callback,
            async_id as f64,
            type_value_handle.get_nanbox_f64(),
            async_id_to_js_number(trigger_async_id),
            resource_handle.get_nanbox_f64(),
        );
    });
}

fn before_with_kind(async_id: u64, trigger_async_id: u64, is_promise: bool) {
    if async_id == 0 {
        return;
    }
    EXECUTION_STACK.with(|stack| {
        stack
            .borrow_mut()
            .push((execution_async_id_u64(), trigger_async_id_u64()));
    });
    CURRENT_EXECUTION_ID.with(|c| c.set(async_id));
    CURRENT_TRIGGER_ID.with(|c| c.set(trigger_async_id));
    with_hook_callbacks(HookPhase::Before, is_promise, |callback| {
        js_closure_call1(callback, async_id as f64);
    });
}

pub fn before(async_id: u64, trigger_async_id: u64) {
    before_with_kind(async_id, trigger_async_id, false);
}

pub fn before_promise(async_id: u64, trigger_async_id: u64) {
    before_with_kind(async_id, trigger_async_id, true);
}

fn after_with_kind(async_id: u64, is_promise: bool) {
    if async_id == 0 {
        return;
    }
    with_hook_callbacks(HookPhase::After, is_promise, |callback| {
        js_closure_call1(callback, async_id as f64);
    });
    let prev = EXECUTION_STACK
        .with(|stack| stack.borrow_mut().pop())
        .unwrap_or((0, 0));
    CURRENT_EXECUTION_ID.with(|c| c.set(prev.0));
    CURRENT_TRIGGER_ID.with(|c| c.set(prev.1));
}

pub fn after(async_id: u64) {
    after_with_kind(async_id, false);
}

pub fn after_promise(async_id: u64) {
    after_with_kind(async_id, true);
}

/// Throw-unwind counterpart of [`after`]: restore the execution/trigger ids
/// of the enclosing scope WITHOUT firing `after` hook callbacks — this runs
/// inside `js_throw` (via a context guard), where re-entering user JS is not
/// safe (#788).
pub(crate) fn unwind_execution_scope() {
    let prev = EXECUTION_STACK
        .with(|stack| stack.borrow_mut().pop())
        .unwrap_or((0, 0));
    CURRENT_EXECUTION_ID.with(|c| c.set(prev.0));
    CURRENT_TRIGGER_ID.with(|c| c.set(prev.1));
}

pub fn promise_resolve(async_id: u64) {
    if async_id == 0 {
        return;
    }
    with_hook_callbacks(HookPhase::PromiseResolve, true, |callback| {
        js_closure_call1(callback, async_id as f64);
    });
}

fn destroy_with_kind(async_id: u64, is_promise: bool) {
    if async_id == 0 {
        return;
    }
    let should_emit = {
        let mut resources = RESOURCES.lock().unwrap();
        match resources.get_mut(&async_id) {
            Some(meta) if !meta.destroyed => {
                meta.destroyed = true;
                true
            }
            Some(_) | None => false,
        }
    };
    if !should_emit {
        return;
    }
    with_hook_callbacks(HookPhase::Destroy, is_promise, |callback| {
        js_closure_call1(callback, async_id as f64);
    });
    RESOURCES.lock().unwrap().remove(&async_id);
}

pub fn destroy(async_id: u64) {
    destroy_with_kind(async_id, false);
}

/// Explicit `AsyncResource.emitDestroy()` notification. Unlike native
/// provider teardown, Node does not make this API idempotent: every call emits
/// a destroy hook for the resource id. Remove the tracked metadata after the
/// first notification, but continue delivering later explicit notifications.
fn emit_explicit_destroy(async_id: u64) {
    if async_id == 0 {
        return;
    }
    with_hook_callbacks(HookPhase::Destroy, false, |callback| {
        js_closure_call1(callback, async_id as f64);
    });
    RESOURCES.lock().unwrap().remove(&async_id);
}

pub fn destroy_promise(async_id: u64) {
    destroy_with_kind(async_id, true);
}

/// Run a synchronous native completion as an observable async-hooks provider.
/// The operation may already have done its blocking work eagerly, but its
/// Promise settlement still needs the same provider execution/resource scope
/// Node gives a libuv completion. The returned JS value is rooted across hook
/// callbacks, which are arbitrary allocating user code.
pub fn run_provider_completion(type_name: &'static str, completion: impl FnOnce() -> f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let resource = crate::object::js_object_alloc_null_proto(0, 0);
    let resource_handle = scope.root_raw_mut_ptr(resource);
    let ids = resource_handle.with_mut_ptr::<ObjectHeader, _>(|resource| {
        init_resource(
            type_name,
            crate::value::js_nanbox_pointer(resource as i64),
            true,
        )
    });
    let outcome = try_run_resource_scope(ids, completion);
    let (threw, result) = match outcome {
        Ok(value) => (false, scope.root_nanbox_f64(value)),
        Err(error) => (true, scope.root_nanbox_f64(error)),
    };
    let destroy_outcome = crate::exception::js_call_catching(|| {
        destroy(ids.async_id);
        TAG_UNDEFINED_F64
    });
    let destroy_error = destroy_outcome
        .err()
        .map(|error| scope.root_nanbox_f64(error));
    if threw {
        crate::exception::js_throw(result.get_nanbox_f64());
    }
    if let Some(error) = destroy_error {
        crate::exception::js_throw(error.get_nanbox_f64());
    }
    result.get_nanbox_f64()
}

/// Enter an existing provider's captured AsyncLocalStorage and execution-id
/// scope for one native callback phase.
pub fn try_enter_resource_scope(ids: AsyncResourceIds) -> Result<(), f64> {
    let context = RESOURCES
        .lock()
        .unwrap()
        .get(&ids.async_id)
        .map(|meta| meta.context.clone())
        .unwrap_or_default();
    let previous = crate::async_context::enter_context(&context);
    crate::async_context::push_context_guard(
        crate::async_context::ContextGuardAction::RestoreSnapshot(previous),
    );
    crate::async_context::push_context_guard(
        crate::async_context::ContextGuardAction::RestoreExecutionIds,
    );
    let outcome = crate::exception::js_call_catching(|| {
        before(ids.async_id, ids.trigger_async_id);
        TAG_UNDEFINED_F64
    });
    if let Err(error) = outcome {
        let scope = crate::gc::RuntimeHandleScope::new();
        let error = scope.root_nanbox_f64(error);
        if let Some(action) = crate::async_context::pop_context_guard() {
            crate::async_context::apply_context_guard(action);
        }
        if let Some(action) = crate::async_context::pop_context_guard() {
            crate::async_context::apply_context_guard(action);
        }
        return Err(error.get_nanbox_f64());
    }
    Ok(())
}

pub fn enter_resource_scope(ids: AsyncResourceIds) {
    if let Err(error) = try_enter_resource_scope(ids) {
        crate::exception::js_throw(error);
    }
}

/// Leave a provider scope entered by [`enter_resource_scope`].
pub fn try_leave_resource_scope(async_id: u64) -> Result<(), f64> {
    let outcome = crate::exception::js_call_catching(|| {
        after(async_id);
        TAG_UNDEFINED_F64
    });
    let scope = crate::gc::RuntimeHandleScope::new();
    let (threw, result) = match outcome {
        Ok(value) => (false, scope.root_nanbox_f64(value)),
        Err(error) => (true, scope.root_nanbox_f64(error)),
    };
    if let Some(action) = crate::async_context::pop_context_guard() {
        if threw {
            crate::async_context::apply_context_guard(action);
        }
    }
    if let Some(action) = crate::async_context::pop_context_guard() {
        crate::async_context::apply_context_guard(action);
    }
    if threw {
        Err(result.get_nanbox_f64())
    } else {
        Ok(())
    }
}

pub fn leave_resource_scope(async_id: u64) {
    if let Err(error) = try_leave_resource_scope(async_id) {
        crate::exception::js_throw(error);
    }
}

pub fn run_resource_scope(ids: AsyncResourceIds, completion: impl FnOnce()) {
    let _ = run_resource_scope_catching(ids, || {
        completion();
        TAG_UNDEFINED_F64
    });
}

/// Execute user code inside an existing provider and return its exception only
/// after the provider context and execution-id stacks have been restored.
pub fn try_run_resource_scope(
    ids: AsyncResourceIds,
    completion: impl FnOnce() -> f64,
) -> Result<f64, f64> {
    try_enter_resource_scope(ids)?;
    let scope = crate::gc::RuntimeHandleScope::new();
    let outcome = crate::exception::js_call_catching(completion);
    let (threw, result) = match outcome {
        Ok(value) => (false, scope.root_nanbox_f64(value)),
        Err(error) => (true, scope.root_nanbox_f64(error)),
    };
    let leave = try_leave_resource_scope(ids.async_id);
    if let Err(error) = leave {
        let error = scope.root_nanbox_f64(error);
        return Err(error.get_nanbox_f64());
    }
    if threw {
        Err(result.get_nanbox_f64())
    } else {
        Ok(result.get_nanbox_f64())
    }
}

pub fn run_resource_scope_catching(ids: AsyncResourceIds, completion: impl FnOnce() -> f64) -> f64 {
    match try_run_resource_scope(ids, completion) {
        Ok(value) => value,
        Err(error) => crate::exception::js_throw(error),
    }
}

pub fn enqueue_gc_destroy(async_id: u64) {
    if async_id != 0 {
        GC_DESTROY_QUEUE.lock().unwrap().push_back(async_id);
    }
}

pub fn drain_gc_destroy_queue() -> i32 {
    let ids: Vec<u64> = {
        let mut q = GC_DESTROY_QUEUE.lock().unwrap();
        q.drain(..).collect()
    };
    let count = ids.len() as i32;
    for id in ids {
        destroy(id);
    }
    count
}

#[inline]
fn async_id_to_js_number(id: u64) -> f64 {
    if id == u64::MAX {
        -1.0
    } else {
        id as f64
    }
}

fn string_header_to_string(ptr: *const StringHeader) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = crate::string::string_data(ptr);
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

fn js_string_value_to_string(value: f64) -> String {
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    string_header_to_string(ptr)
}

fn symbol_to_string(value: f64) -> String {
    if unsafe { crate::symbol::js_is_symbol(value) == 0 } {
        return "Symbol()".to_string();
    }
    let ptr = unsafe { crate::symbol::js_symbol_to_string(value) } as *const StringHeader;
    string_header_to_string(ptr)
}

fn value_is_array(value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let ptr = jv.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return false;
    }
    unsafe {
        let gc_header = &*(ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader);
        gc_header.obj_type == crate::gc::GC_TYPE_ARRAY
    }
}

fn is_callable_value(value: f64) -> bool {
    !crate::fs::extract_closure_ptr(value).is_null()
}

fn describe_received_async_hooks(value: f64) -> String {
    if is_callable_value(value) {
        return "function ".to_string();
    }
    if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        return format!("type symbol ({})", symbol_to_string(value));
    }
    crate::fs::validate::describe_received(value)
}

fn require_string_arg(arg_name: &str, value: f64) -> String {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_any_string() {
        let message = format!(
            "The \"{}\" argument must be of type string. Received {}",
            arg_name,
            describe_received_async_hooks(value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    js_string_value_to_string(value)
}

fn format_js_number_for_error(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_string()
    } else if value == f64::INFINITY {
        "Infinity".to_string()
    } else if value == f64::NEG_INFINITY {
        "-Infinity".to_string()
    } else if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}

const MAX_SAFE_JS_INTEGER: f64 = 9_007_199_254_740_991.0;

fn trigger_async_id_value(value: f64) -> Option<u64> {
    let jv = JSValue::from_bits(value.to_bits());
    let id = if jv.is_int32() {
        jv.as_int32() as f64
    } else if jv.is_number() {
        jv.as_number()
    } else {
        return None;
    };

    if !id.is_finite() || id.fract() != 0.0 || !(-1.0..=MAX_SAFE_JS_INTEGER).contains(&id) {
        return None;
    }
    if id == -1.0 {
        Some(u64::MAX)
    } else {
        Some(id as u64)
    }
}

fn render_invalid_trigger_async_id(value: f64) -> String {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return "undefined".to_string();
    }
    if jv.is_null() {
        return "null".to_string();
    }
    if jv.is_bool() {
        return jv.as_bool().to_string();
    }
    if jv.is_any_string() {
        return js_string_value_to_string(value);
    }
    if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        return symbol_to_string(value);
    }
    if jv.is_int32() {
        return jv.as_int32().to_string();
    }
    if jv.is_number() {
        return format_js_number_for_error(jv.as_number());
    }
    if value_is_array(value) {
        return "[]".to_string();
    }
    if jv.is_pointer() {
        return "{}".to_string();
    }
    "undefined".to_string()
}

fn trigger_async_id_or_throw(value: f64) -> u64 {
    if let Some(id) = trigger_async_id_value(value) {
        return id;
    }
    let message = format!(
        "Invalid triggerAsyncId value: {}",
        render_invalid_trigger_async_id(value)
    );
    crate::fs::validate::throw_range_error_named(&message, "ERR_INVALID_ASYNC_ID")
}

fn throw_null_trigger_async_id_options() -> ! {
    let message = b"Cannot read properties of null (reading 'triggerAsyncId')";
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn trigger_id_from_options(options: f64) -> u64 {
    let options_value = JSValue::from_bits(options.to_bits());
    if options_value.is_undefined() {
        return execution_async_id_u64();
    }
    if options_value.is_int32() || options_value.is_number() {
        return trigger_async_id_or_throw(options);
    }
    if options_value.is_null() {
        throw_null_trigger_async_id_options();
    }

    // Node's constructor first validates the option and then consumes it,
    // making an accessor observable twice. Preserve that exact ordering; the
    // `requireManualDestroy` option is read after both trigger-id reads.
    let first_trigger_value = object_field(options, b"triggerAsyncId");
    if !JSValue::from_bits(first_trigger_value.to_bits()).is_undefined() {
        let _ = trigger_async_id_or_throw(first_trigger_value);
    }
    let trigger_value = object_field(options, b"triggerAsyncId");
    let trigger_value_kind = JSValue::from_bits(trigger_value.to_bits());
    let trigger_id = if trigger_value_kind.is_undefined() {
        execution_async_id_u64()
    } else {
        trigger_async_id_or_throw(trigger_value)
    };
    let _ = object_field(options, b"requireManualDestroy");
    trigger_id
}

fn render_apply_value(value: f64) -> String {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return "undefined".to_string();
    }
    if jv.is_null() {
        return "null".to_string();
    }
    if jv.is_bool() {
        return jv.as_bool().to_string();
    }
    if jv.is_any_string() {
        return js_string_value_to_string(value);
    }
    if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        return symbol_to_string(value);
    }
    if jv.is_int32() {
        return jv.as_int32().to_string();
    }
    if jv.is_number() {
        return format_js_number_for_error(jv.as_number());
    }
    if value_is_array(value) {
        return "[object Array]".to_string();
    }
    if jv.is_pointer() {
        return "#<Object>".to_string();
    }
    "undefined".to_string()
}

fn describe_apply_type(value: f64) -> &'static str {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        "undefined"
    } else if jv.is_null() {
        "null"
    } else if jv.is_bool() {
        "a boolean"
    } else if jv.is_any_string() {
        "a string"
    } else if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        "a symbol"
    } else if jv.is_int32() || jv.is_number() {
        "a number"
    } else {
        "an object"
    }
}

fn throw_apply_not_function(value: f64) -> ! {
    let message = format!(
        "Function.prototype.apply was called on {}, which is {} and not a function",
        render_apply_value(value),
        describe_apply_type(value)
    );
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn validate_bind_callback(value: f64) {
    if is_callable_value(value) {
        return;
    }
    let message = format!(
        "The \"fn\" argument must be of type function. Received {}",
        describe_received_async_hooks(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

#[no_mangle]
pub extern "C" fn js_async_resource_new(type_value: f64, options: f64) -> i64 {
    new_async_resource_with_public_value(type_value, options, None)
}

fn new_async_resource_with_public_value(
    type_value: f64,
    options: f64,
    public_resource: Option<f64>,
) -> i64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let type_handle = scope.root_nanbox_f64(type_value);
    let options_handle = scope.root_nanbox_f64(options);
    let type_name = require_string_arg("type", type_handle.get_nanbox_f64());
    if type_name.is_empty() && hooks_active() {
        crate::fs::validate::throw_type_error_with_code(
            "The \"type\" argument must be a non-empty string",
            "ERR_ASYNC_TYPE",
        );
    }
    let trigger_async_id = trigger_id_from_options(options_handle.get_nanbox_f64());
    // The public resource object is the constructor handle itself. Allocate it
    // before firing init so the fourth callback argument is already the exact
    // object returned by `new AsyncResource(...)`, as it is in Node.
    let handle = Box::into_raw(Box::new(AsyncResourceHandle {
        ids: AsyncResourceIds {
            async_id: 0,
            trigger_async_id,
        },
        event_emitter: 0,
    })) as i64;
    ASYNC_RESOURCE_HANDLES.lock().unwrap().insert(handle);
    ASYNC_RESOURCE_HANDLE_COUNT.fetch_add(1, Ordering::Relaxed);
    let resource_value = public_resource.unwrap_or_else(|| crate::value::js_nanbox_pointer(handle));
    let ids = init_resource_with_trigger(&type_name, resource_value, true, trigger_async_id);
    unsafe { (*(handle as *mut AsyncResourceHandle)).ids = ids };
    handle
}

/// Initialize the native backing for a source-compiled
/// `class X extends AsyncResource` while keeping the public subclass object as
/// the resource passed to hooks and returned by `executionAsyncResource()`.
#[no_mangle]
pub extern "C" fn js_async_resource_subclass_init(
    this_value: f64,
    type_value: f64,
    options: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let this_handle = scope.root_nanbox_f64(this_value);
    let type_handle = scope.root_nanbox_f64(type_value);
    let options_handle = scope.root_nanbox_f64(options);
    let backing = new_async_resource_with_public_value(
        type_handle.get_nanbox_f64(),
        options_handle.get_nanbox_f64(),
        Some(this_handle.get_nanbox_f64()),
    );
    let raw =
        crate::value::js_nanbox_get_pointer(this_handle.get_nanbox_f64()) as *mut ObjectHeader;
    if !raw.is_null() && crate::value::addr_class::is_plausible_heap_addr(raw as usize) {
        let key = js_string_from_bytes(
            ASYNC_RESOURCE_SUBCLASS_KEY.as_ptr(),
            ASYNC_RESOURCE_SUBCLASS_KEY.len() as u32,
        );
        let raw =
            crate::value::js_nanbox_get_pointer(this_handle.get_nanbox_f64()) as *mut ObjectHeader;
        crate::object::js_object_set_field_by_name(
            raw,
            key,
            crate::value::js_nanbox_pointer(backing),
        );
        for (name, length) in [
            ("asyncId", 0),
            ("triggerAsyncId", 0),
            ("emitDestroy", 0),
            ("runInAsyncScope", 2),
            ("bind", 2),
        ] {
            let method = if name == "bind" {
                async_resource_bind_method_value(backing)
            } else {
                crate::object::async_resource_prototype_method_value(name, length)
            };
            let method_handle = scope.root_nanbox_f64(method);
            let method_key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
            let current_this = this_handle.get_nanbox_f64();
            let current_raw =
                crate::value::js_nanbox_get_pointer(current_this) as *mut ObjectHeader;
            crate::object::js_object_set_field_by_name(
                current_raw,
                method_key,
                method_handle.get_nanbox_f64(),
            );
            crate::object::set_builtin_property_attrs(
                current_raw as usize,
                name.to_string(),
                crate::object::PropertyAttrs::new(true, false, true),
            );
        }
    }
    this_handle.get_nanbox_f64()
}

/// Link the backing AsyncResource owned by EventEmitterAsyncResource to its
/// public emitter. Node exposes this as `emitter.asyncResource.eventEmitter`.
/// Both sides are stable native handles, so the link does not need GC rooting.
pub fn set_async_resource_event_emitter(handle: i64, event_emitter: i64) {
    if handle == 0 || !ASYNC_RESOURCE_HANDLES.lock().unwrap().contains(&handle) {
        return;
    }
    unsafe { (*(handle as *mut AsyncResourceHandle)).event_emitter = event_emitter };
}

#[no_mangle]
pub extern "C" fn js_async_resource_set_event_emitter(handle: i64, event_emitter: i64) {
    set_async_resource_event_emitter(handle, event_emitter);
}

/// Node exposes `AsyncResource#bind` as an instance-bound function: unlike
/// the other prototype methods, extracting it and calling it later keeps the
/// originating resource as its receiver.  Use a dedicated rest trampoline
/// instead of the generic class-method reifier, whose ordinary semantics use
/// the call-site `this` for a detached function.
extern "C" fn async_resource_bind_method_trampoline(
    closure: *const ClosureHeader,
    rest: f64,
) -> f64 {
    if closure.is_null() {
        return TAG_UNDEFINED_F64;
    }
    let handle = js_closure_get_capture_ptr(closure, 0);
    let scope = crate::gc::RuntimeHandleScope::new();
    let args_array =
        scope.root_raw_const_ptr(crate::value::js_nanbox_get_pointer(rest) as *const ArrayHeader);
    let args_len = if args_array.get_raw_const_ptr::<ArrayHeader>().is_null() {
        0
    } else {
        js_array_length(args_array.get_raw_const_ptr())
    };
    let callback = if args_len == 0 {
        TAG_UNDEFINED_F64
    } else {
        crate::array::js_array_get_f64(args_array.get_raw_const_ptr(), 0)
    };
    let callback = scope.root_nanbox_f64(callback);
    let this_arg = if args_len < 2 {
        TAG_UNDEFINED_F64
    } else {
        crate::array::js_array_get_f64(args_array.get_raw_const_ptr(), 1)
    };
    let this_arg = scope.root_nanbox_f64(this_arg);
    let bound =
        js_async_resource_bind(handle, callback.get_nanbox_f64(), this_arg.get_nanbox_f64());
    if bound == 0 {
        TAG_UNDEFINED_F64
    } else {
        crate::value::js_nanbox_pointer(bound)
    }
}

fn async_resource_bind_method_value(handle: i64) -> f64 {
    let trampoline = async_resource_bind_method_trampoline as *const u8;
    js_register_closure_rest(trampoline, 0);
    let closure = js_closure_alloc(trampoline, 1);
    if closure.is_null() {
        return TAG_UNDEFINED_F64;
    }
    js_closure_set_capture_ptr(closure, 0, handle);
    crate::object::set_builtin_closure_length(closure as usize, 2);
    crate::object::set_bound_native_closure_name(closure, "bind");
    crate::value::js_nanbox_pointer(closure as i64)
}

pub fn try_async_resource_property_dispatch(handle: i64, property: &str) -> Option<f64> {
    if ASYNC_RESOURCE_HANDLE_COUNT.load(Ordering::Relaxed) == 0
        || !ASYNC_RESOURCE_HANDLES.lock().unwrap().contains(&handle)
    {
        return None;
    }
    // User-defined own properties shadow AsyncResource.prototype just as they
    // do on Node's ordinary public resource object.  The backing allocation is
    // a native Box, so keep expandos in the same traced side table used by
    // small native handles rather than ever treating it as an ObjectHeader.
    if let Some(value) = crate::object::handle_expando::handle_expando_get(handle, property) {
        return Some(value);
    }
    if property == "bind" {
        return Some(async_resource_bind_method_value(handle));
    }
    if let Some((name, length)) = match property {
        "asyncId" => Some(("asyncId", 0)),
        "triggerAsyncId" => Some(("triggerAsyncId", 0)),
        "emitDestroy" => Some(("emitDestroy", 0)),
        "runInAsyncScope" => Some(("runInAsyncScope", 2)),
        _ => None,
    } {
        return Some(crate::object::async_resource_prototype_method_value(
            name, length,
        ));
    }
    if property != "eventEmitter" {
        return None;
    }
    let emitter = unsafe { (*(handle as *const AsyncResourceHandle)).event_emitter };
    Some(if emitter == 0 {
        TAG_UNDEFINED_F64
    } else {
        crate::value::js_nanbox_pointer(emitter)
    })
}

/// Dynamic method dispatch for `AsyncResource` receivers whose static type
/// the codegen lost (closure-captured / `any`-typed bindings). Registry
/// membership is checked before any dereference, so a genuine heap object
/// can never be claimed. Returns `None` when the receiver is not a live
/// AsyncResource handle or the method name is not part of its vocabulary.
pub fn try_async_resource_method_dispatch(
    receiver: i64,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if ASYNC_RESOURCE_HANDLE_COUNT.load(Ordering::Relaxed) == 0 {
        return None;
    }
    if !matches!(
        method_name,
        "runInAsyncScope" | "asyncId" | "triggerAsyncId" | "emitDestroy" | "bind"
    ) {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let raw_args: Vec<f64> = if args_ptr.is_null() || args_len == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(args_ptr, args_len).to_vec() }
    };
    let arg_handles = scope.root_nanbox_f64_slice(&raw_args);
    let receiver = scope.root_raw_mut_ptr(receiver as *mut ObjectHeader);
    let handle = resolve_async_resource_handle(receiver.get_raw_mut_ptr::<ObjectHeader>() as i64)?;
    let handle = scope.root_raw_const_ptr(handle as *const AsyncResourceHandle);
    let args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    Some(match method_name {
        "asyncId" => {
            js_async_resource_async_id(handle.get_raw_const_ptr::<AsyncResourceHandle>() as i64)
        }
        "triggerAsyncId" => js_async_resource_trigger_async_id(
            handle.get_raw_const_ptr::<AsyncResourceHandle>() as i64,
        ),
        "emitDestroy" => {
            js_async_resource_emit_destroy(handle.get_raw_const_ptr::<AsyncResourceHandle>() as i64);
            crate::value::js_nanbox_pointer(receiver.get_raw_mut_ptr::<ObjectHeader>() as i64)
        }
        "runInAsyncScope" => {
            // runInAsyncScope(fn[, thisArg, ...args])
            let callback = args.first().copied().unwrap_or(TAG_UNDEFINED_F64);
            let this_arg = args.get(1).copied().unwrap_or(TAG_UNDEFINED_F64);
            let rest = if args.len() > 2 { &args[2..] } else { &[] };
            let args_array = pack_rest_args_array(rest);
            js_async_resource_run_in_async_scope(
                handle.get_raw_const_ptr::<AsyncResourceHandle>() as i64,
                callback,
                this_arg,
                args_array,
            )
        }
        "bind" => {
            // bind(fn[, thisArg])
            let callback = args.first().copied().unwrap_or(TAG_UNDEFINED_F64);
            let this_arg = args.get(1).copied().unwrap_or(TAG_UNDEFINED_F64);
            let bound = js_async_resource_bind(
                handle.get_raw_const_ptr::<AsyncResourceHandle>() as i64,
                callback,
                this_arg,
            );
            if bound == 0 {
                TAG_UNDEFINED_F64
            } else {
                crate::value::js_nanbox_pointer(bound)
            }
        }
        _ => unreachable!("gated by the matches! above"),
    })
}

/// Pack trailing call args into a fresh array for the `args_array: i64`
/// FFI convention (`0` = no forwarded args).
fn pack_rest_args_array(rest: &[f64]) -> i64 {
    if rest.is_empty() {
        return 0;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let rest_handles = scope.root_nanbox_f64_slice(rest);
    let arr = crate::array::js_array_alloc(0);
    let arr_handle = scope.root_raw_mut_ptr(arr);
    for handle in &rest_handles {
        let grown =
            crate::array::js_array_push_f64(arr_handle.get_raw_mut_ptr(), handle.get_nanbox_f64());
        arr_handle.set_raw_mut_ptr(grown);
    }
    arr_handle.get_raw_mut_ptr::<ArrayHeader>() as i64
}

#[no_mangle]
pub extern "C" fn js_async_resource_async_id(handle: i64) -> f64 {
    let Some(handle) = resolve_async_resource_handle(handle) else {
        return 0.0;
    };
    let resource = unsafe { &*(handle as *const AsyncResourceHandle) };
    resource.ids.async_id as f64
}

#[no_mangle]
pub extern "C" fn js_async_resource_trigger_async_id(handle: i64) -> f64 {
    let Some(handle) = resolve_async_resource_handle(handle) else {
        return 0.0;
    };
    let resource = unsafe { &*(handle as *const AsyncResourceHandle) };
    async_id_to_js_number(resource.ids.trigger_async_id)
}

#[no_mangle]
pub extern "C" fn js_async_resource_emit_destroy(handle: i64) -> i64 {
    if let Some(backing) = resolve_async_resource_handle(handle) {
        let resource = unsafe { &*(backing as *const AsyncResourceHandle) };
        emit_explicit_destroy(resource.ids.async_id);
    }
    handle
}

#[no_mangle]
pub extern "C" fn js_async_resource_run_in_async_scope(
    handle: i64,
    callback_value: f64,
    this_arg: f64,
    args_array: i64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver_handle = scope.root_raw_mut_ptr(handle as *mut ObjectHeader);
    let callback_handle = scope.root_nanbox_f64(callback_value);
    let this_arg_handle = scope.root_nanbox_f64(this_arg);
    let args_array_handle = scope.root_raw_const_ptr(args_array as *const ArrayHeader);
    let receiver = receiver_handle.get_raw_mut_ptr::<ObjectHeader>() as i64;
    let Some(handle) = resolve_async_resource_handle(receiver) else {
        return TAG_UNDEFINED_F64;
    };
    let handle = scope.root_raw_const_ptr(handle as *const AsyncResourceHandle);
    if !is_callable_value(callback_handle.get_nanbox_f64()) {
        throw_apply_not_function(callback_handle.get_nanbox_f64());
    }
    let ids = unsafe { (*handle.get_raw_const_ptr::<AsyncResourceHandle>()).ids };
    let rebound_bits = crate::closure::clone_closure_rebind_this(
        callback_handle.get_nanbox_f64().to_bits(),
        this_arg_handle.get_nanbox_f64(),
    );
    let rebound_handle = scope.root_nanbox_f64(f64::from_bits(rebound_bits));
    if crate::fs::extract_closure_ptr(rebound_handle.get_nanbox_f64()).is_null() {
        throw_apply_not_function(callback_handle.get_nanbox_f64());
    }
    let outcome = try_run_resource_scope(ids, || {
        let callback = crate::fs::extract_closure_ptr(rebound_handle.get_nanbox_f64());
        let previous_this = scope.root_nanbox_f64(crate::object::js_implicit_this_set(
            this_arg_handle.get_nanbox_f64(),
        ));
        let callback_outcome = crate::exception::js_call_catching(|| {
            if args_array_handle
                .get_raw_const_ptr::<ArrayHeader>()
                .is_null()
            {
                unsafe { js_closure_call_array(callback as i64, ptr::null(), 0) }
            } else {
                let arr = args_array_handle.get_raw_const_ptr::<ArrayHeader>();
                let len = js_array_length(arr) as i64;
                let data = unsafe {
                    (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64
                };
                unsafe { js_closure_call_array(callback as i64, data, len) }
            }
        });
        crate::object::js_implicit_this_set(previous_this.get_nanbox_f64());
        match callback_outcome {
            Ok(value) => value,
            Err(error) => crate::exception::js_throw(error),
        }
    });
    match outcome {
        Ok(value) => value,
        Err(error) => crate::exception::js_throw(error),
    }
}

/// Trampoline body for `AsyncResource#bind`. Stored as the `func_ptr` of the
/// synthesized closure; receives the rest array of forwarded args and replays
/// the call through `runInAsyncScope` so init/before/after/destroy fire with
/// the bound resource's async id active.
extern "C" fn async_resource_bind_trampoline(closure: *const ClosureHeader, rest: f64) -> f64 {
    if closure.is_null() {
        return TAG_UNDEFINED_F64;
    }
    let handle = js_closure_get_capture_ptr(closure, 0);
    let callback = js_closure_get_capture_f64(closure, 1);
    let mut this_arg = js_closure_get_capture_f64(closure, 2);
    if handle == 0 {
        return TAG_UNDEFINED_F64;
    }
    if JSValue::from_bits(this_arg.to_bits()).is_undefined() {
        this_arg = crate::object::js_implicit_this_get();
    }
    let args_array_ptr = ptr_from_nanboxed(rest) as i64;
    js_async_resource_run_in_async_scope(handle, callback, this_arg, args_array_ptr)
}

fn register_bind_trampoline_once() {
    thread_local! {
        // CLOSURE_REST_REGISTRY is thread-local, so each thread that
        // synthesizes a bind() trampoline must register the func_ptr once.
        static REGISTERED: Cell<bool> = const { Cell::new(false) };
    }
    REGISTERED.with(|flag| {
        if !flag.get() {
            // fixed_arity=0 → dispatch_rest_bundled calls
            // `f(closure, rest_array)` regardless of forwarded arity.
            js_register_closure_rest(async_resource_bind_trampoline as *const u8, 0);
            flag.set(true);
        }
    });
}

#[no_mangle]
pub extern "C" fn js_async_resource_bind(handle: i64, callback_value: f64, this_arg: f64) -> i64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver_handle = scope.root_raw_mut_ptr(handle as *mut ObjectHeader);
    let callback_handle = scope.root_nanbox_f64(callback_value);
    let this_arg_handle = scope.root_nanbox_f64(this_arg);
    validate_bind_callback(callback_handle.get_nanbox_f64());
    let Some(handle) =
        resolve_async_resource_handle(receiver_handle.get_raw_mut_ptr::<ObjectHeader>() as i64)
    else {
        return 0;
    };
    let handle = scope.root_raw_const_ptr(handle as *const AsyncResourceHandle);
    register_bind_trampoline_once();
    let closure = js_closure_alloc(async_resource_bind_trampoline as *const u8, 3);
    if closure.is_null() {
        return 0;
    }
    let closure_handle = scope.root_raw_mut_ptr(closure);
    js_closure_set_capture_ptr(
        closure_handle.get_raw_mut_ptr(),
        0,
        handle.get_raw_const_ptr::<AsyncResourceHandle>() as i64,
    );
    js_closure_set_capture_f64(
        closure_handle.get_raw_mut_ptr(),
        1,
        callback_handle.get_nanbox_f64(),
    );
    js_closure_set_capture_f64(
        closure_handle.get_raw_mut_ptr(),
        2,
        this_arg_handle.get_nanbox_f64(),
    );
    if let Some(length) = crate::closure::closure_length(crate::fs::extract_closure_ptr(
        callback_handle.get_nanbox_f64(),
    )) {
        crate::object::set_builtin_closure_length(
            closure_handle.get_raw_mut_ptr::<ClosureHeader>() as usize,
            length,
        );
    }
    crate::object::set_bound_native_closure_name(
        closure_handle.get_raw_mut_ptr::<ClosureHeader>(),
        "bound",
    );
    closure_handle.get_raw_mut_ptr::<ClosureHeader>() as i64
}

#[no_mangle]
pub extern "C" fn js_async_resource_static_bind(callback: i64, type_value: f64) -> i64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback_handle = scope.root_raw_const_ptr(callback as *const ClosureHeader);
    let callback_value = if callback_handle
        .get_raw_const_ptr::<ClosureHeader>()
        .is_null()
    {
        TAG_UNDEFINED_F64
    } else {
        box_ptr(callback_handle.get_raw_const_ptr::<ClosureHeader>() as *const u8)
    };
    let callback_value_handle = scope.root_nanbox_f64(callback_value);
    let type_handle = scope.root_nanbox_f64(type_value);
    let bound = js_async_resource_static_bind_value(
        callback_value_handle.get_nanbox_f64(),
        type_handle.get_nanbox_f64(),
        TAG_UNDEFINED_F64,
    );
    ptr_from_nanboxed(bound) as i64
}

pub extern "C" fn js_async_resource_static_bind_value(
    callback_value: f64,
    type_value: f64,
    this_arg: f64,
) -> f64 {
    validate_bind_callback(callback_value);
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback_handle = scope.root_nanbox_f64(callback_value);
    let type_value = if JSValue::from_bits(type_value.to_bits()).is_undefined() {
        let callback = crate::fs::extract_closure_ptr(callback_handle.get_nanbox_f64());
        let inferred = if callback.is_null() {
            None
        } else {
            let own_name = crate::closure::closure_get_dynamic_prop(callback as usize, "name");
            let own_name = JSValue::from_bits(own_name.to_bits());
            if own_name.is_any_string() {
                let name = js_string_value_to_string(f64::from_bits(own_name.bits()));
                (!name.is_empty()).then_some(name)
            } else {
                unsafe { crate::builtins::function_name_for_ptr((*callback).func_ptr as usize) }
                    .filter(|name| !name.is_empty())
            }
        };
        let default_type = inferred.as_deref().unwrap_or("bound-anonymous-fn");
        box_string(
            js_string_from_bytes(default_type.as_ptr(), default_type.len() as u32) as *const u8,
        )
    } else {
        type_value
    };
    let type_handle = scope.root_nanbox_f64(type_value);
    let this_arg_handle = scope.root_nanbox_f64(this_arg);
    let handle = js_async_resource_new(type_handle.get_nanbox_f64(), TAG_UNDEFINED_F64);
    let bound = js_async_resource_bind(
        handle,
        callback_handle.get_nanbox_f64(),
        this_arg_handle.get_nanbox_f64(),
    );
    if bound == 0 {
        TAG_UNDEFINED_F64
    } else {
        crate::value::js_nanbox_pointer(bound)
    }
}

#[no_mangle]
pub extern "C" fn js_async_resource_static_bind_direct(
    callback_value: f64,
    type_value: f64,
    this_arg: f64,
    _rest: i64,
) -> f64 {
    js_async_resource_static_bind_value(callback_value, type_value, this_arg)
}

pub extern "C" fn js_async_resource_static_bind_method(
    _closure: *const ClosureHeader,
    callback_value: f64,
    type_value: f64,
    this_arg: f64,
    _rest: f64,
) -> f64 {
    js_async_resource_static_bind_value(callback_value, type_value, this_arg)
}

pub extern "C" fn js_async_local_storage_static_bind_method(
    _closure: *const ClosureHeader,
    callback_value: f64,
    _rest: f64,
) -> f64 {
    js_async_resource_static_bind_value(callback_value, TAG_UNDEFINED_F64, TAG_UNDEFINED_F64)
}

#[no_mangle]
pub extern "C" fn js_async_local_storage_static_bind_direct(
    callback_value: f64,
    _rest: i64,
) -> f64 {
    js_async_resource_static_bind_value(callback_value, TAG_UNDEFINED_F64, TAG_UNDEFINED_F64)
}

fn register_context_snapshot(snapshot: crate::async_context::AsyncContextSnapshot) -> usize {
    let id = NEXT_CONTEXT_SNAPSHOT_ID.fetch_add(1, Ordering::Relaxed);
    CONTEXT_SNAPSHOTS.lock().unwrap().insert(id, snapshot);
    id
}

fn run_with_context_snapshot(snapshot_id: usize, f: impl FnOnce() -> f64) -> f64 {
    let snapshot = CONTEXT_SNAPSHOTS
        .lock()
        .unwrap()
        .get(&snapshot_id)
        .cloned()
        .unwrap_or_default();
    let scope = crate::gc::RuntimeHandleScope::new();
    let mut snapshot = snapshot;
    let snapshot_roots = crate::async_context::root_snapshot(&scope, &snapshot);
    let previous = crate::async_context::enter_context(&snapshot);
    // Guard-held (GC-scanned, throw-safe) — see runInAsyncScope (#788).
    crate::async_context::push_context_guard(
        crate::async_context::ContextGuardAction::RestoreSnapshot(previous),
    );
    let result = f();
    let result_handle = scope.root_nanbox_f64(result);
    crate::async_context::refresh_snapshot_from_roots(&mut snapshot, &snapshot_roots);
    if let Some(action) = crate::async_context::pop_context_guard() {
        crate::async_context::apply_context_guard(action);
    }
    result_handle.get_nanbox_f64()
}

fn call_callback_with_rest(callback_value: f64, this_arg: f64, rest: f64) -> f64 {
    if !is_callable_value(callback_value) {
        throw_apply_not_function(callback_value);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback_handle = scope.root_nanbox_f64(callback_value);
    let this_arg_handle = scope.root_nanbox_f64(this_arg);
    let rebound_bits = crate::closure::clone_closure_rebind_this(
        callback_handle.get_nanbox_f64().to_bits(),
        this_arg_handle.get_nanbox_f64(),
    );
    let rebound_handle = scope.root_nanbox_f64(f64::from_bits(rebound_bits));
    let callback = crate::fs::extract_closure_ptr(rebound_handle.get_nanbox_f64());
    if callback.is_null() {
        throw_apply_not_function(callback_handle.get_nanbox_f64());
    }
    let args_array = ptr_from_nanboxed(rest) as *const ArrayHeader;
    let args_array_handle = scope.root_raw_const_ptr(args_array);
    let prev_this = crate::object::js_implicit_this_set(this_arg_handle.get_nanbox_f64());
    let result = if args_array.is_null() {
        unsafe { js_closure_call_array(callback as i64, ptr::null(), 0) }
    } else {
        let arr = args_array_handle.get_raw_const_ptr::<ArrayHeader>();
        let len = js_array_length(arr) as i64;
        let data = if arr.is_null() || len == 0 {
            ptr::null()
        } else {
            unsafe { (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64 }
        };
        unsafe { js_closure_call_array(callback as i64, data, len) }
    };
    crate::object::js_implicit_this_set(prev_this);
    result
}

extern "C" fn async_local_storage_snapshot_trampoline(
    closure: *const ClosureHeader,
    callback_value: f64,
    rest: f64,
) -> f64 {
    let snapshot_id = js_closure_get_capture_ptr(closure, 0) as usize;
    run_with_context_snapshot(snapshot_id, || {
        // AsyncLocalStorage.snapshot() intentionally invokes the supplied
        // callback as a plain function. The receiver used to call the snapshot
        // wrapper itself is not forwarded.
        call_callback_with_rest(callback_value, TAG_UNDEFINED_F64, rest)
    })
}

fn register_snapshot_trampoline_once() {
    thread_local! {
        static REGISTERED: Cell<bool> = const { Cell::new(false) };
    }
    REGISTERED.with(|flag| {
        if !flag.get() {
            js_register_closure_rest(async_local_storage_snapshot_trampoline as *const u8, 1);
            flag.set(true);
        }
    });
}

fn async_local_storage_static_snapshot_value() -> f64 {
    register_snapshot_trampoline_once();
    let snapshot_id = register_context_snapshot(crate::async_context::capture_context());
    let closure = js_closure_alloc(async_local_storage_snapshot_trampoline as *const u8, 1);
    if closure.is_null() {
        return TAG_UNDEFINED_F64;
    }
    js_closure_set_capture_ptr(closure, 0, snapshot_id as i64);
    crate::object::set_builtin_closure_length(closure as usize, 1);
    crate::object::set_bound_native_closure_name(closure, "bound");
    crate::value::js_nanbox_pointer(closure as i64)
}

pub extern "C" fn js_async_local_storage_static_snapshot_method(
    _closure: *const ClosureHeader,
    _rest: f64,
) -> f64 {
    async_local_storage_static_snapshot_value()
}

#[no_mangle]
pub extern "C" fn js_async_local_storage_static_snapshot_direct(_rest: i64) -> f64 {
    async_local_storage_static_snapshot_value()
}

pub fn scan_async_hooks_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_async_hooks_roots_mut(&mut visitor);
}

pub fn scan_async_hooks_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut hooks = HOOKS.lock().unwrap();
    for hook in hooks.iter_mut() {
        visitor.visit_raw_const_ptr_slot(&mut hook.callbacks.init);
        visitor.visit_raw_const_ptr_slot(&mut hook.callbacks.before);
        visitor.visit_raw_const_ptr_slot(&mut hook.callbacks.after);
        visitor.visit_raw_const_ptr_slot(&mut hook.callbacks.destroy);
        visitor.visit_raw_const_ptr_slot(&mut hook.callbacks.promise_resolve);
    }
    drop(hooks);
    let mut resources = RESOURCES.lock().unwrap();
    for meta in resources.values_mut() {
        // Resource identity is weak: the resource's owning scheduler/promise
        // keeps it alive, while its finalizer enqueues the destroy event and
        // removes this metadata. Marking the value here made PROMISE entries
        // immortal and forced the runtime to fake destroy-at-settlement.
        visitor.visit_metadata_nanbox_f64_slot(&mut meta.resource);
        crate::async_context::scan_snapshot_roots_mut(&mut meta.context, visitor);
    }
    drop(resources);

    let mut snapshots = CONTEXT_SNAPSHOTS.lock().unwrap();
    for snapshot in snapshots.values_mut() {
        crate::async_context::scan_snapshot_roots_mut(snapshot, visitor);
    }
    drop(snapshots);

    let mut providers_bits = ASYNC_WRAP_PROVIDERS.load(Ordering::Relaxed);
    if providers_bits != 0 {
        visitor.visit_nanbox_u64_slot(&mut providers_bits);
        ASYNC_WRAP_PROVIDERS.store(providers_bits, Ordering::Relaxed);
    }

    let mut top_level_bits = TOP_LEVEL_RESOURCE.load(Ordering::Relaxed);
    if top_level_bits != 0 {
        visitor.visit_nanbox_u64_slot(&mut top_level_bits);
        TOP_LEVEL_RESOURCE.store(top_level_bits, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod test_support;
#[cfg(test)]
pub use test_support::reset_for_tests;
#[cfg(test)]
pub(crate) use test_support::{
    test_async_hooks_scanner_snapshot, test_seed_async_hooks_scanner_roots,
};
