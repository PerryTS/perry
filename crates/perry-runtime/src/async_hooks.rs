//! Native async_hooks lifecycle support.
//!
//! This module owns the process-wide hook list, async resource ids, and the
//! thread-local execution/trigger id stack used by the compiled runtime.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::ptr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex};

use crate::array::{js_array_length, ArrayHeader};
use crate::closure::{
    js_closure_alloc, js_closure_call1, js_closure_call4, js_closure_get_capture_f64,
    js_closure_get_capture_ptr, js_closure_set_capture_f64, js_closure_set_capture_ptr,
    ClosureHeader,
};
use crate::object::{js_object_get_field_by_name, ObjectHeader};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::{JSValue, POINTER_MASK};

mod arg_values;
use arg_values::{
    async_id_to_js_number, is_callable_value, js_string_value_to_string, require_string_arg,
    throw_apply_not_function, trigger_id_from_options, validate_bind_callback,
};
mod provider_ffi;
pub use provider_ffi::{
    defer_destroy_after_check_turns, js_async_hooks_provider_defer_destroy,
    js_async_hooks_provider_destroy, js_async_hooks_provider_enter, js_async_hooks_provider_init,
    js_async_hooks_provider_init_with_trigger, js_async_hooks_provider_leave,
    js_async_hooks_provider_run_catching, js_async_hooks_provider_run_catching_deferred_destroy,
    js_async_hooks_provider_run_catching_deferred_destroy_on_error,
    js_async_hooks_provider_run_catching_with_this,
};
mod scopes;
pub use scopes::{
    enter_resource_scope, leave_resource_scope, run_provider_completion, run_resource_scope,
    run_resource_scope_catching, try_enter_resource_scope, try_leave_resource_scope,
    try_run_resource_scope,
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
    occupied: bool,
    order: u64,
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
    // #10522: aHash, not SipHash — every `setTimeout` inserts and every clear
    // or fire removes an entry, keyed by our own monotonic async id, so there
    // is no untrusted key to defend against and SipHash's rounds were a
    // measurable share of timer churn.
    static RESOURCES: LazyLock<Mutex<HashMap<u64, ResourceMeta, ahash::RandomState>>> =
        LazyLock::new(|| Mutex::new(HashMap::default()));
    static GC_DESTROY_QUEUE: Mutex<VecDeque<u64>> = Mutex::new(VecDeque::new());
    static NEXT_CONTEXT_SNAPSHOT_ID: AtomicUsize = AtomicUsize::new(1);
    static CONTEXT_SNAPSHOTS: LazyLock<
        Mutex<HashMap<usize, crate::async_context::AsyncContextSnapshot>>,
    > = LazyLock::new(|| Mutex::new(HashMap::new()));
    static ASYNC_WRAP_PROVIDERS: AtomicU64 = AtomicU64::new(0);
}

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

pub struct AsyncHookPayload {
    index: usize,
}

pub struct AsyncResourcePayload {
    ids: AsyncResourceIds,
}

impl Drop for AsyncHookPayload {
    fn drop(&mut self) {
        if self.index != usize::MAX {
            retire_hook(self.index);
        }
    }
}

impl Drop for AsyncResourcePayload {
    fn drop(&mut self) {
        if self.ids.async_id != 0 {
            RESOURCES.lock().unwrap().remove(&self.ids.async_id);
        }
    }
}

static ASYNC_HOOK_FAMILY: crate::native_payload::NativePayloadFamily =
    crate::native_payload::NativePayloadFamily {
        class_id: ASYNC_HOOK_CLASS_ID,
        name: "AsyncHook",
        constructor_export: None,
        constructor_length: 1,
        links_owner: false,
        install_prototype: install_async_hook_prototype,
    };

static ASYNC_RESOURCE_FAMILY: crate::native_payload::NativePayloadFamily =
    crate::native_payload::NativePayloadFamily {
        class_id: ASYNC_RESOURCE_CLASS_ID,
        name: "AsyncResource",
        constructor_export: Some(("async_hooks", "AsyncResource")),
        constructor_length: 2,
        links_owner: false,
        install_prototype: install_async_resource_prototype,
    };

// AsyncResource and AsyncHook are ordinary GC objects. Their ObjectMeta owns
// a typed native-payload cell; no recognition registry or raw Box address is
// exposed to JavaScript.

/// Legacy reserved id, already used by `instanceof` and
/// `class_registry::parent_static`. NOT moved into the `0xFFFF_24xx` block:
/// it is baked into emitted code in three places and renumbering a live class
/// id is #10824's hazard for no gain.
pub(crate) const ASYNC_RESOURCE_CLASS_ID: u32 = crate::native_class_ids::ASYNC_RESOURCE_LEGACY;
/// A fresh id from the `native_class_ids` web-builtin block (`0x2411`, the
/// next one after the `perry/tui` family's `0x240B..=0x2410`).
pub(crate) const ASYNC_HOOK_CLASS_ID: u32 = crate::native_class_ids::ASYNC_HOOK;

pub(crate) fn install_async_resource_prototype(
    proto: &mut crate::native_payload::PayloadPrototype,
) {
    proto.method(
        "runInAsyncScope",
        crate::fn_info!(async_resource_run_thunk, 3; with_rest(2), with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
        2,
    );
    proto.method(
        "emitDestroy",
        crate::fn_info!(async_resource_destroy_thunk, 0; with_declared(0), with_flags(crate::closure::FN_BUILTIN)),
        0,
    );
    proto.method(
        "asyncId",
        crate::fn_info!(async_resource_id_thunk, 0; with_declared(0), with_flags(crate::closure::FN_BUILTIN)),
        0,
    );
    proto.method(
        "triggerAsyncId",
        crate::fn_info!(async_resource_trigger_id_thunk, 0; with_declared(0), with_flags(crate::closure::FN_BUILTIN)),
        0,
    );
    proto.method(
        "bind",
        crate::fn_info!(async_resource_bind_thunk, 2; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
        2,
    );
}

fn canonical_async_resource_prototype() -> *mut ObjectHeader {
    let proto = crate::value::js_nanbox_get_pointer(crate::object::async_resource_prototype_value())
        as *mut ObjectHeader;
    adopt_async_resource_prototype(proto)
}

pub(crate) fn adopt_async_resource_prototype(proto: *mut ObjectHeader) -> *mut ObjectHeader {
    crate::native_payload::adopt_prototype(&ASYNC_RESOURCE_FAMILY, proto)
}

fn install_async_hook_prototype(proto: &mut crate::native_payload::PayloadPrototype) {
    proto.method(
        "enable",
        crate::fn_info!(async_hook_enable_thunk, 0; with_declared(0), with_flags(crate::closure::FN_BUILTIN)),
        0,
    );
    proto.method(
        "disable",
        crate::fn_info!(async_hook_disable_thunk, 0; with_declared(0), with_flags(crate::closure::FN_BUILTIN)),
        0,
    );
}

extern "C" fn async_hook_enable_thunk(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    js_async_hook_enable(crate::value::js_nanbox_get_pointer(this.as_f64()))
}

extern "C" fn async_hook_disable_thunk(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    js_async_hook_disable(crate::value::js_nanbox_get_pointer(this.as_f64()))
}

extern "C" fn async_resource_id_thunk(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    js_async_resource_async_id(crate::value::js_nanbox_get_pointer(this.as_f64()))
}

extern "C" fn async_resource_trigger_id_thunk(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    js_async_resource_trigger_async_id(crate::value::js_nanbox_get_pointer(this.as_f64()))
}

extern "C" fn async_resource_destroy_thunk(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    js_async_resource_emit_destroy(crate::value::js_nanbox_get_pointer(this.as_f64()))
}

extern "C" fn async_resource_run_thunk(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    callback: f64,
    this_arg: f64,
    rest: f64,
) -> f64 {
    js_async_resource_run_in_async_scope(
        crate::value::js_nanbox_get_pointer(this.as_f64()),
        callback,
        this_arg,
        crate::value::js_nanbox_get_pointer(rest),
    )
}

extern "C" fn async_resource_bind_thunk(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    callback: f64,
    this_arg: f64,
) -> f64 {
    let bound = js_async_resource_bind(
        crate::value::js_nanbox_get_pointer(this.as_f64()),
        callback,
        this_arg,
    );
    if bound == 0 {
        TAG_UNDEFINED_F64
    } else {
        crate::value::js_nanbox_pointer(bound)
    }
}

unsafe fn resource_payload(receiver: i64) -> Option<&'static mut AsyncResourcePayload> {
    crate::native_payload::payload_mut_attached::<AsyncResourcePayload>(
        crate::value::js_nanbox_pointer(receiver),
        &ASYNC_RESOURCE_FAMILY,
    )
    .ok()
}

unsafe fn hook_payload(receiver: i64) -> Option<&'static mut AsyncHookPayload> {
    crate::native_payload::payload_mut_attached::<AsyncHookPayload>(
        crate::value::js_nanbox_pointer(receiver),
        &ASYNC_HOOK_FAMILY,
    )
    .ok()
}

pub(crate) fn resolve_async_resource_handle(receiver: i64) -> Option<i64> {
    unsafe { resource_payload(receiver).map(|_| receiver) }
}

#[cfg(test)]
pub(crate) fn test_force_next_async_resource_resolve_gc() {
    TEST_FORCE_RESOLVE_GC.store(1, Ordering::Relaxed);
}

/// Link a subclass receiver to its backing exactly the way
/// `js_async_resource_subclass_init` does: the held `__perryAsyncResourceBacking`
/// own property AND the `ObjectMeta.native_state` word the resolver actually
/// reads. Planting only one of the two would let the GC-root test below pass
/// against a representation production does not produce.
#[cfg(test)]
pub(crate) fn test_link_async_resource_subclass(
    receiver: *mut ObjectHeader,
    backing: i64,
) -> *mut ObjectHeader {
    let ids = unsafe { resource_payload(backing) }
        .map(|payload| payload.ids)
        .unwrap_or(AsyncResourceIds {
            async_id: 0,
            trigger_async_id: 0,
        });
    let value = crate::value::js_nanbox_pointer(receiver as i64);
    crate::native_payload::attach_to_object(
        value,
        &ASYNC_RESOURCE_FAMILY,
        AsyncResourcePayload { ids },
        0,
    );
    crate::value::js_nanbox_get_pointer(value) as *mut ObjectHeader
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

fn save_hook_state(handle: f64, callbacks: HookCallbacks, track_promises: bool) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let values = [
        callbacks.init,
        callbacks.before,
        callbacks.after,
        callbacks.destroy,
        callbacks.promise_resolve,
    ]
    .map(|callback| {
        scope.root_nanbox_f64(if callback.is_null() {
            TAG_UNDEFINED_F64
        } else {
            box_ptr(callback as *const u8)
        })
    });
    let state = scope.root_nanbox_f64(crate::native_payload::js_state(
        handle,
        &ASYNC_HOOK_FAMILY,
        true,
    ));
    let names: [&[u8]; 5] = [b"init", b"before", b"after", b"destroy", b"promiseResolve"];
    for (name, value) in names.into_iter().zip(values.iter()) {
        let key = scope.root_string_ptr(js_string_from_bytes(name.as_ptr(), name.len() as u32));
        let obj = ptr_from_nanboxed(state.get_nanbox_f64()) as *mut ObjectHeader;
        key.with_const_ptr::<StringHeader, _>(|key| {
            crate::object::js_object_set_field_by_name(obj, key, value.get_nanbox_f64())
        });
    }
    let key = scope.root_string_ptr(js_string_from_bytes(
        b"trackPromises".as_ptr(),
        b"trackPromises".len() as u32,
    ));
    let obj = ptr_from_nanboxed(state.get_nanbox_f64()) as *mut ObjectHeader;
    key.with_const_ptr::<StringHeader, _>(|key| {
        crate::object::js_object_set_field_by_name(
            obj,
            key,
            f64::from_bits(if track_promises {
                crate::value::TAG_TRUE
            } else {
                crate::value::TAG_FALSE
            }),
        )
    });
}

fn callbacks_from_hook_state(handle: f64) -> Option<(HookCallbacks, bool)> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let state = scope.root_nanbox_f64(crate::native_payload::js_state(
        handle,
        &ASYNC_HOOK_FAMILY,
        false,
    ));
    if JSValue::from_bits(state.get_nanbox_f64().to_bits()).is_undefined() {
        return None;
    }
    let init = scope.root_nanbox_f64(object_field(state.get_nanbox_f64(), b"init"));
    let before = scope.root_nanbox_f64(object_field(state.get_nanbox_f64(), b"before"));
    let after = scope.root_nanbox_f64(object_field(state.get_nanbox_f64(), b"after"));
    let destroy = scope.root_nanbox_f64(object_field(state.get_nanbox_f64(), b"destroy"));
    let promise_resolve =
        scope.root_nanbox_f64(object_field(state.get_nanbox_f64(), b"promiseResolve"));
    let track_promises =
        scope.root_nanbox_f64(object_field(state.get_nanbox_f64(), b"trackPromises"));
    Some((
        HookCallbacks {
            init: closure_from_value(init.get_nanbox_f64()),
            before: closure_from_value(before.get_nanbox_f64()),
            after: closure_from_value(after.get_nanbox_f64()),
            destroy: closure_from_value(destroy.get_nanbox_f64()),
            promise_resolve: closure_from_value(promise_resolve.get_nanbox_f64()),
        },
        track_promises.get_nanbox_f64().to_bits() == crate::value::TAG_TRUE,
    ))
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
pub extern "C" fn js_async_hooks_create_hook(options: f64) -> f64 {
    validate_create_hook_options(options);
    let (callbacks, track_promises) = callbacks_from_options(options);
    let scope = crate::gc::RuntimeHandleScope::new();
    let callbacks = [
        callbacks.init,
        callbacks.before,
        callbacks.after,
        callbacks.destroy,
        callbacks.promise_resolve,
    ]
    .map(|callback| {
        scope.root_nanbox_f64(if callback.is_null() {
            TAG_UNDEFINED_F64
        } else {
            box_ptr(callback as *const u8)
        })
    });
    let value = scope.root_nanbox_f64(crate::native_payload::alloc(
        &ASYNC_HOOK_FAMILY,
        AsyncHookPayload { index: usize::MAX },
        0,
        &[],
    ));
    save_hook_state(
        value.get_nanbox_f64(),
        HookCallbacks {
            init: closure_from_value(callbacks[0].get_nanbox_f64()),
            before: closure_from_value(callbacks[1].get_nanbox_f64()),
            after: closure_from_value(callbacks[2].get_nanbox_f64()),
            destroy: closure_from_value(callbacks[3].get_nanbox_f64()),
            promise_resolve: closure_from_value(callbacks[4].get_nanbox_f64()),
        },
        track_promises,
    );
    if crate::hot_diag::receiver_repr_on() {
        crate::hot_diag::receiver_repr_note_constructed(
            crate::hot_diag::ReceiverReprFamily::AsyncHook,
        );
    }
    value.get_nanbox_f64()
}

fn register_hook(callbacks: HookCallbacks, track_promises: bool) -> usize {
    let mut hooks = HOOKS.lock().unwrap();
    let order = hooks
        .iter()
        .filter(|record| record.occupied)
        .map(|record| record.order)
        .max()
        .unwrap_or(0)
        + 1;
    let index = hooks
        .iter()
        .position(|record| !record.occupied)
        .unwrap_or(hooks.len());
    let record = HookRecord {
        callbacks,
        enabled: false,
        track_promises,
        occupied: true,
        order,
    };
    if index == hooks.len() {
        hooks.push(record);
    } else {
        hooks[index] = record;
    }
    index
}

fn ensure_async_hook_index(receiver: i64) -> Option<usize> {
    let value = crate::value::js_nanbox_pointer(receiver);
    let needs_attach = match unsafe {
        crate::native_payload::payload_mut_attached::<AsyncHookPayload>(value, &ASYNC_HOOK_FAMILY)
    } {
        Ok(payload) if payload.index != usize::MAX => return Some(payload.index),
        // createHook already owns an OPEN payload whose record is unpublished.
        // Initialize that payload in place: attach rejects OPEN cells, and
        // dropping its rejected input would retire the record we just made.
        Ok(_) => false,
        Err(crate::native_payload::PayloadMiss::Closed) => true,
        Err(crate::native_payload::PayloadMiss::Foreign) => return None,
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(value);
    let (callbacks, track_promises) = callbacks_from_hook_state(receiver.get_nanbox_f64())?;
    let index = register_hook(callbacks, track_promises);
    if needs_attach {
        if !crate::native_payload::attach_to_object(
            receiver.get_nanbox_f64(),
            &ASYNC_HOOK_FAMILY,
            AsyncHookPayload { index },
            0,
        ) {
            return None;
        }
    } else {
        let payload = unsafe {
            crate::native_payload::payload_mut_attached::<AsyncHookPayload>(
                receiver.get_nanbox_f64(),
                &ASYNC_HOOK_FAMILY,
            )
        };
        let Ok(payload) = payload else {
            retire_hook(index);
            return None;
        };
        payload.index = index;
    }
    Some(index)
}

#[no_mangle]
pub extern "C" fn js_async_hook_enable(receiver: i64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(receiver));
    let Some(index) = ensure_async_hook_index(crate::value::js_nanbox_get_pointer(
        receiver.get_nanbox_f64(),
    )) else {
        return receiver.get_nanbox_f64();
    };
    if HOOK_CALLBACK_DEPTH.with(Cell::get) != 0 {
        PENDING_HOOK_STATES.with(|pending| {
            pending.borrow_mut().insert(index, true);
        });
        return receiver.get_nanbox_f64();
    }
    set_hook_enabled(index, true);
    receiver.get_nanbox_f64()
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

fn retire_hook(index: usize) {
    let mut hooks = HOOKS.lock().unwrap();
    let Some(record) = hooks.get_mut(index) else {
        return;
    };
    if record.enabled && record.callbacks.has_any() {
        HOOKS_ACTIVE.fetch_sub(1, Ordering::Relaxed);
        if record.track_promises {
            PROMISE_HOOKS_ACTIVE.fetch_sub(1, Ordering::Relaxed);
        }
    }
    record.enabled = false;
    record.callbacks = HookCallbacks::empty();
    record.occupied = false;
    while hooks.last().is_some_and(|record| !record.occupied) {
        hooks.pop();
    }
}

#[no_mangle]
pub extern "C" fn js_async_hook_disable(receiver: i64) -> f64 {
    let Some(index) = unsafe { hook_payload(receiver) }.map(|payload| payload.index) else {
        return crate::value::js_nanbox_pointer(receiver);
    };
    if HOOK_CALLBACK_DEPTH.with(Cell::get) != 0 {
        PENDING_HOOK_STATES.with(|pending| {
            pending.borrow_mut().insert(index, false);
        });
        // The current delivery cascade retains its original membership until
        // the outermost callback returns. Its record then retires, while the
        // native payload itself can be released immediately.
        unsafe { hook_payload(receiver) }.unwrap().index = usize::MAX;
    } else {
        set_hook_enabled(index, false);
    }
    crate::native_payload::close(
        crate::value::js_nanbox_pointer(receiver),
        &ASYNC_HOOK_FAMILY,
    );
    crate::value::js_nanbox_pointer(receiver)
}

fn enabled_callbacks(is_promise: bool) -> Vec<HookCallbacks> {
    if !hooks_active() {
        return Vec::new();
    }
    let hooks = HOOKS.lock().unwrap();
    let mut callbacks: Vec<_> = hooks
        .iter()
        .filter(|hook| hook.enabled && (!is_promise || hook.track_promises))
        .map(|hook| (hook.order, hook.callbacks))
        .collect();
    // Reusing a retired delivery slot must not change Node's enable order.
    callbacks.sort_unstable_by_key(|(order, _)| *order);
    callbacks
        .into_iter()
        .map(|(_, callbacks)| callbacks)
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
            if enabled {
                set_hook_enabled(index, true);
            } else {
                retire_hook(index);
            }
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
    let ids = init_resource_metadata(type_name, resource, force_allocate, trigger_async_id);
    if ids.async_id != 0 {
        emit_init(ids.async_id, type_name, trigger_async_id, resource);
    }
    ids
}

fn init_resource_metadata(
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

    AsyncResourceIds {
        async_id,
        trigger_async_id,
    }
}

fn emit_init(async_id: u64, type_name: &str, trigger_async_id: u64, resource: f64) {
    // `with_hook_callbacks` returns at once without hooks; check first so the
    // common no-hooks case does not allocate the type-name string per resource
    // (one per `setTimeout`, #10522).
    if !hooks_active() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let resource_handle = scope.root_nanbox_f64(resource);
    let type_ptr = js_string_from_bytes(type_name.as_ptr(), type_name.len() as u32);
    let type_value_handle = scope.root_nanbox_f64(box_string(type_ptr as *const u8));
    with_hook_callbacks(HookPhase::Init, type_name == "PROMISE", |callback| {
        js_closure_call4(
            callback,
            crate::closure::plain_call_receiver(),
            async_id as f64,
            type_value_handle.get_nanbox_f64(),
            async_id_to_js_number(trigger_async_id),
            resource_handle.get_nanbox_f64(),
        );
    });
}

/// Temporarily publish an AsyncResource object through
/// `executionAsyncResource()`. The RESOURCES table must not permanently root
/// the object it describes, or the object's payload finalizer can never run.
fn swap_resource_value(async_id: u64, value: f64) -> f64 {
    let mut resources = RESOURCES.lock().unwrap();
    let Some(meta) = resources.get_mut(&async_id) else {
        return TAG_UNDEFINED_F64;
    };
    std::mem::replace(&mut meta.resource, value)
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
        js_closure_call1(
            callback,
            crate::closure::plain_call_receiver(),
            async_id as f64,
        );
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
        js_closure_call1(
            callback,
            crate::closure::plain_call_receiver(),
            async_id as f64,
        );
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
        js_closure_call1(
            callback,
            crate::closure::plain_call_receiver(),
            async_id as f64,
        );
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
        js_closure_call1(
            callback,
            crate::closure::plain_call_receiver(),
            async_id as f64,
        );
    });
    RESOURCES.lock().unwrap().remove(&async_id);
}

pub fn destroy(async_id: u64) {
    destroy_with_kind(async_id, false);
}

/// Explicit `AsyncResource.emitDestroy()` notification. Unlike native
/// provider teardown, Node does not make this API idempotent: every call emits
/// a destroy hook for the resource id. Keep the captured context metadata so
/// the still-live object remains reusable; its payload Drop removes the entry.
fn emit_explicit_destroy(async_id: u64) {
    if async_id == 0 {
        return;
    }
    with_hook_callbacks(HookPhase::Destroy, false, |callback| {
        js_closure_call1(
            callback,
            crate::closure::plain_call_receiver(),
            async_id as f64,
        );
    });
}

pub fn destroy_promise(async_id: u64) {
    destroy_with_kind(async_id, true);
}

pub fn enqueue_gc_destroy(async_id: u64) {
    if async_id != 0 {
        GC_DESTROY_QUEUE.lock().unwrap().push_back(async_id);
    }
}

pub(crate) fn gc_destroy_work_pending() -> bool {
    !GC_DESTROY_QUEUE.lock().unwrap().is_empty()
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

#[no_mangle]
pub extern "C" fn js_async_resource_new(type_value: f64, options: f64) -> f64 {
    new_async_resource_with_public_value(type_value, options, None)
}

fn new_async_resource_with_public_value(
    type_value: f64,
    options: f64,
    public_resource: Option<f64>,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let type_handle = scope.root_nanbox_f64(type_value);
    let options_handle = scope.root_nanbox_f64(options);
    let public_resource = public_resource.map(|value| scope.root_nanbox_f64(value));
    let type_name = require_string_arg("type", type_handle.get_nanbox_f64());
    if type_name.is_empty() && hooks_active() {
        crate::fs::validate::throw_type_error_with_code(
            "The \"type\" argument must be a non-empty string",
            "ERR_ASYNC_TYPE",
        );
    }
    let trigger_async_id = trigger_id_from_options(options_handle.get_nanbox_f64());
    let payload = AsyncResourcePayload {
        ids: AsyncResourceIds {
            async_id: 0,
            trigger_async_id,
        },
    };
    if crate::hot_diag::receiver_repr_on() {
        crate::hot_diag::receiver_repr_note_constructed(
            crate::hot_diag::ReceiverReprFamily::AsyncResource,
        );
    }
    let public = scope.root_nanbox_f64(match public_resource {
        Some(owner) => {
            crate::native_payload::attach_to_object(
                owner.get_nanbox_f64(),
                &ASYNC_RESOURCE_FAMILY,
                payload,
                0,
            );
            owner.get_nanbox_f64()
        }
        None => {
            let proto = canonical_async_resource_prototype();
            crate::native_payload::alloc_with_prototype(
                &ASYNC_RESOURCE_FAMILY,
                payload,
                0,
                &[],
                proto,
            )
        }
    });
    let ids = init_resource_metadata(&type_name, public.get_nanbox_f64(), true, trigger_async_id);
    let public_raw = crate::value::js_nanbox_get_pointer(public.get_nanbox_f64());
    if let Some(payload) = unsafe { resource_payload(public_raw) } {
        payload.ids = ids;
    }
    emit_init(
        ids.async_id,
        &type_name,
        trigger_async_id,
        public.get_nanbox_f64(),
    );
    // `emit_init` above received the public object directly. Outside an
    // active runInAsyncScope it must not remain a strong registry root: the
    // ordinary object owns the metadata lifetime through its payload Drop.
    let _ = swap_resource_value(ids.async_id, TAG_UNDEFINED_F64);
    public.get_nanbox_f64()
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
    let _ = canonical_async_resource_prototype();
    let _ = new_async_resource_with_public_value(
        type_handle.get_nanbox_f64(),
        options_handle.get_nanbox_f64(),
        Some(this_handle.get_nanbox_f64()),
    );
    this_handle.get_nanbox_f64()
}

/// Link the AsyncResource owned by EventEmitterAsyncResource to its public
/// emitter. This is JS-visible state, so it belongs in an ordinary traced own
/// property rather than in the native payload.
pub fn set_async_resource_event_emitter(resource: i64, event_emitter: i64) {
    if resolve_async_resource_handle(resource).is_none() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let resource = scope.root_raw_mut_ptr(resource as *mut ObjectHeader);
    let emitter = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(event_emitter));
    let key = scope.root_string_ptr(js_string_from_bytes(b"eventEmitter".as_ptr(), 12));
    key.with_const_ptr::<StringHeader, _>(|key| {
        resource.with_mut_ptr::<ObjectHeader, _>(|obj| {
            crate::object::define_builtin_data_property(
                obj,
                key as *mut StringHeader,
                emitter.get_nanbox_f64(),
                "eventEmitter".to_string(),
                crate::object::PropertyAttrs::new(true, false, true),
            )
        })
    });
}

#[no_mangle]
pub extern "C" fn js_async_resource_set_event_emitter(handle: i64, event_emitter: i64) {
    set_async_resource_event_emitter(handle, event_emitter);
}

#[no_mangle]
pub extern "C" fn js_async_resource_async_id(handle: i64) -> f64 {
    let Some(handle) = resolve_async_resource_handle(handle) else {
        return TAG_UNDEFINED_F64;
    };
    unsafe { resource_payload(handle) }
        .map(|resource| resource.ids.async_id as f64)
        .unwrap_or(TAG_UNDEFINED_F64)
}

#[no_mangle]
pub extern "C" fn js_async_resource_trigger_async_id(handle: i64) -> f64 {
    let Some(handle) = resolve_async_resource_handle(handle) else {
        return TAG_UNDEFINED_F64;
    };
    unsafe { resource_payload(handle) }
        .map(|resource| async_id_to_js_number(resource.ids.trigger_async_id))
        .unwrap_or(TAG_UNDEFINED_F64)
}

#[no_mangle]
pub extern "C" fn js_async_resource_emit_destroy(handle: i64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(handle));
    let id = resolve_async_resource_handle(handle).and_then(|backing| {
        unsafe { resource_payload(backing) }.map(|resource| resource.ids.async_id)
    });
    if let Some(id) = id {
        emit_explicit_destroy(id);
    }
    receiver.get_nanbox_f64()
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
    // #10926: the forced collection used to sit INSIDE
    // `resolve_async_resource_handle`, because that resolver allocated the
    // `__perryAsyncResourceBacking` key and therefore had inputs of its own to
    // root. It reads `ObjectMeta.native_state` now and cannot allocate, so a
    // collection can no longer originate there and forcing one inside it would
    // only be testing scaffolding -- and would hand the resolver a stale
    // receiver, since nothing refreshes it. The axis that still exists is this
    // frame's: a collection between rooting the receiver and resolving it must
    // not lose the receiver. `with_mut_ptr` below refreshes from the root, so
    // the resolve still finds the backing; drop the rooting and it does not.
    #[cfg(test)]
    if TEST_FORCE_RESOLVE_GC.swap(0, Ordering::Relaxed) != 0 {
        let _ = crate::gc::gc_collect_minor();
    }
    let Some(handle) = receiver_handle
        .with_mut_ptr::<ObjectHeader, _>(|receiver| resolve_async_resource_handle(receiver as i64))
    else {
        return TAG_UNDEFINED_F64;
    };
    if !is_callable_value(callback_handle.get_nanbox_f64()) {
        throw_apply_not_function(callback_handle.get_nanbox_f64());
    }
    let ids = unsafe { resource_payload(handle) }.unwrap().ids;
    let public_resource = receiver_handle.with_mut_ptr::<ObjectHeader, _>(|receiver| {
        crate::value::js_nanbox_pointer(receiver as i64)
    });
    let previous_resource =
        scope.root_nanbox_f64(swap_resource_value(ids.async_id, public_resource));
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
        let callback_outcome = crate::exception::js_call_catching(|| {
            args_array_handle.with_const_ptr::<ArrayHeader, _>(|arr| {
                if arr.is_null() {
                    unsafe {
                        crate::closure::js_closure_call_array(
                            callback as i64,
                            crate::closure::JsThis::from_f64(this_arg_handle.get_nanbox_f64()),
                            ptr::null(),
                            0,
                        )
                    }
                } else {
                    let len = js_array_length(arr) as i64;
                    let data = unsafe {
                        crate::array::array_elements_ptr(arr as *const ArrayHeader) as *const f64
                    };
                    unsafe {
                        crate::closure::js_closure_call_array(
                            callback as i64,
                            crate::closure::JsThis::from_f64(this_arg_handle.get_nanbox_f64()),
                            data,
                            len,
                        )
                    }
                }
            })
        });
        match callback_outcome {
            Ok(value) => value,
            Err(error) => crate::exception::js_throw(error),
        }
    });
    let _ = swap_resource_value(ids.async_id, previous_resource.get_nanbox_f64());
    match outcome {
        Ok(value) => value,
        Err(error) => crate::exception::js_throw(error),
    }
}

/// Trampoline body for `AsyncResource#bind`. Stored as the `func_ptr` of the
/// synthesized closure; receives the rest array of forwarded args and replays
/// the call through `runInAsyncScope` so init/before/after/destroy fire with
/// the bound resource's async id active.
extern "C" fn async_resource_bind_trampoline(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    rest: f64,
) -> f64 {
    if closure.is_null() {
        return TAG_UNDEFINED_F64;
    }
    let resource = js_closure_get_capture_f64(closure, 0);
    let handle = crate::value::js_nanbox_get_pointer(resource);
    let callback = js_closure_get_capture_f64(closure, 1);
    let mut this_arg = js_closure_get_capture_f64(closure, 2);
    if handle == 0 {
        return TAG_UNDEFINED_F64;
    }
    if JSValue::from_bits(this_arg.to_bits()).is_undefined() {
        this_arg = this.as_f64();
    }
    let args_array_ptr = ptr_from_nanboxed(rest) as i64;
    js_async_resource_run_in_async_scope(handle, callback, this_arg, args_array_ptr)
}

fn register_bind_trampoline_once() {
    thread_local! {
        // The closure body registry is thread-local, so each thread that
        // synthesizes a bind() trampoline must register the func_ptr once.
        static REGISTERED: Cell<bool> = const { Cell::new(false) };
    }
    REGISTERED.with(|flag| {
        if !flag.get() {
            // fixed_arity=0 → dispatch_rest_bundled calls
            // `f(closure, rest_array)` regardless of forwarded arity.
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
    let Some(handle) = receiver_handle
        .with_mut_ptr::<ObjectHeader, _>(|receiver| resolve_async_resource_handle(receiver as i64))
    else {
        return 0;
    };
    register_bind_trampoline_once();
    let closure = js_closure_alloc(
        crate::fn_info!(async_resource_bind_trampoline, 1; with_rest(0)),
        3,
    );
    if closure.is_null() {
        return 0;
    }
    let closure_handle = scope.root_raw_mut_ptr(closure);
    js_closure_set_capture_f64(
        closure_handle.get_raw_mut_ptr(),
        0,
        crate::value::js_nanbox_pointer(handle),
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
                unsafe { crate::builtins::function_name_for_ptr((*callback).code() as usize) }
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
    let resource = js_async_resource_new(type_handle.get_nanbox_f64(), TAG_UNDEFINED_F64);
    let handle = crate::value::js_nanbox_get_pointer(resource);
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
    _this: crate::closure::JsThis,
    callback_value: f64,
    type_value: f64,
    this_arg: f64,
    _rest: f64,
) -> f64 {
    js_async_resource_static_bind_value(callback_value, type_value, this_arg)
}

pub extern "C" fn js_async_local_storage_static_bind_method(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
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

mod context_snapshots;
pub use context_snapshots::{
    js_async_local_storage_static_snapshot_direct, js_async_local_storage_static_snapshot_method,
};

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

// #11471: thread-exit release of this module's process-global tables.
mod thread_exit;
pub(crate) use thread_exit::release_async_hooks_in_freed_ranges;
#[doc(hidden)]
pub use thread_exit::{
    cached_singletons_for_test, clear_cached_singletons_for_test, context_snapshot_holds_for_test,
    hook_callback_registered_for_test, resource_tracked_for_test,
};

#[cfg(test)]
mod test_support;
#[cfg(test)]
pub use test_support::reset_for_tests;
#[cfg(test)]
pub(crate) use test_support::{
    test_async_hooks_scanner_snapshot, test_seed_async_hooks_scanner_roots,
};
