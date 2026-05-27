//! Native bindings for Node's `events` module — `EventEmitter`
//! with the Node-compatible listener-table surface.
//!
//! First wrapper port that exercises perry-ffi's GC-root-scanner
//! surface (added in v0.5.546). User closures passed to
//! `emitter.on(event, cb)` live inside an `EventEmitterHandle`
//! value in the registry; without an explicit mutable GC scanner, a
//! malloc-triggered GC between `.on()` and `.emit()` would sweep the
//! closure or leave copied-minor forwarding pointers stale (issue #35
//! pattern).
//!
//! Issue #850 — rewrote the listener storage to match Node semantics
//! (per-event ordered `Vec<Listener>` with `once` flag, insertion-order
//! event-name shadow, max-listeners ceiling, pending `events.once`
//! promises). Added the previously-missing `.once` / `.addListener` /
//! `.prependListener` / `.prependOnceListener` / `.listeners` /
//! `.rawListeners` / `.eventNames` / `.setMaxListeners` /
//! `.getMaxListeners` instance methods plus the module-level
//! `events.once` / `events.getEventListeners` / `events.listenerCount` /
//! `events.getMaxListeners` / `events.setMaxListeners` helpers.

use perry_ffi::{
    gc_register_mutable_root_scanner_named, get_handle_mut, iter_handles_of_mut, js_array_alloc,
    js_array_get, js_array_push, js_array_set, nanbox_string_bits, read_string, register_handle,
    ArrayHeader, GcRootVisitor, Handle, JsClosure, JsPromise, JsString, JsValue, ObjectHeader,
    Promise, RawClosureHeader, StringHeader,
};
use std::collections::HashMap;

const MIN_HEAP_POINTER: u64 = 0x1000;
const EVENT_TARGET_MIN_HEAP_POINTER: u64 = 0x10000;
const MAX_HEAP_POINTER: u64 = 0x0000_FFFF_FFFF_FFFF;

// Direct hook into perry-runtime's sync Promise resolve.
//
// `JsPromise::resolve_*` route through `perry_ffi_promise_resolve_bits`
// which calls `async_bridge::queue_promise_resolution` — that requires
// the perry-stdlib pump to be registered before the resolution is
// applied to the Promise. `events.once(em, name)` followed by a
// synchronous `em.emit(...)` and a synchronous `await p` doesn't go
// through any perry-stdlib spawn helper, so the pump is never
// registered and the await hangs forever waiting for a state change
// that's stuck in the deferred queue.
//
// Resolving synchronously instead — same path perry-stdlib's
// `js_promise_resolved(value)` uses — settles the Promise immediately,
// matches the `then`/`await` ordering Node expects, and sidesteps the
// pump-registration coupling entirely.
extern "C" {
    fn js_promise_resolve(promise: *mut Promise, value: f64);
    fn js_promise_reject(promise: *mut Promise, reason: f64);
    // #1557: closure allocation hooks needed by events.on's queue-listener.
    // Mirrors perry-runtime::closure exports; declared here because perry-ffi
    // doesn't yet expose them and perry-ext-events deliberately avoids a
    // direct perry-runtime dep.
    fn js_closure_alloc(fn_ptr: *const u8, capture_count: u32) -> *mut RawClosureHeader;
    fn js_closure_set_capture_ptr(closure: *mut RawClosureHeader, slot: u32, ptr: i64);
    fn js_closure_get_capture_ptr(closure: *const RawClosureHeader, slot: u32) -> i64;
    fn js_array_push_f64(arr: *mut ArrayHeader, value: f64) -> *mut ArrayHeader;
    // #1557: AbortSignal listener attachment for events.addAbortListener.
    fn js_string_from_bytes(data: *const u8, len: u32) -> *mut StringHeader;
    fn js_object_get_field_by_name_f64(obj: *const ObjectHeader, key: *const StringHeader) -> f64;
    fn js_abort_signal_add_listener(signal: *mut u8, event: f64, listener: f64);
    fn js_event_target_is_event_target(target: *const u8) -> i32;
    fn js_event_target_get_event_listeners(
        target: *mut u8,
        event: *const StringHeader,
    ) -> *mut ArrayHeader;
    fn js_event_target_get_max_listeners(target: *mut u8) -> f64;
    fn js_event_target_set_max_listeners(target: *mut u8, n: f64) -> i32;
    fn js_abort_signal_remove_listener(signal: *mut u8, event: f64, listener: f64);
    fn js_abort_signal_is_aborted(signal: *mut u8) -> i32;
    fn js_abort_error_value() -> f64;
    fn js_throw(value: f64) -> !;
}

const TAG_UNDEFINED_F64_BITS: u64 = 0x7FFC_0000_0000_0001;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

#[inline]
fn nanbox_pointer_bits(ptr: i64) -> f64 {
    f64::from_bits(POINTER_TAG | ((ptr as u64) & POINTER_MASK))
}

/// One registered listener: a raw closure pointer (i64 to satisfy
/// Send + Sync — the underlying ClosureHeader is GC-managed) plus a
/// `once` flag.
#[derive(Copy, Clone)]
struct Listener {
    callback: i64,
    once: bool,
}

#[derive(Copy, Clone)]
struct PendingOnce {
    promise: *mut Promise,
    signal: f64,
    abort_listener: i64,
}

/// EventEmitter handle with Node-compatible listener-table semantics
/// (issue #850).
pub struct EventEmitterHandle {
    events: HashMap<String, Vec<Listener>>,
    event_order: Vec<String>,
    pending_once_promises: HashMap<String, Vec<PendingOnce>>,
    max_listeners: i32,
}

// SAFETY: `*mut Promise` is not Send/Sync by default, but the registry's
// mutable GC scanner visits pending promise slots so they survive minor
// GC cycles and are rewritten after copied-minor evacuation.
unsafe impl Send for EventEmitterHandle {}
unsafe impl Sync for EventEmitterHandle {}

impl Default for EventEmitterHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl EventEmitterHandle {
    pub fn new() -> Self {
        EventEmitterHandle {
            events: HashMap::new(),
            event_order: Vec::new(),
            pending_once_promises: HashMap::new(),
            // Node's default — `getMaxListeners()` on a fresh emitter
            // returns 10.
            max_listeners: 10,
        }
    }

    fn note_event(&mut self, name: &str) {
        if !self.events.contains_key(name) {
            self.event_order.push(name.to_string());
        }
    }

    fn prune_event_if_empty(&mut self, name: &str) {
        let drop_it = match self.events.get(name) {
            Some(v) => v.is_empty(),
            None => true,
        };
        if drop_it {
            self.events.remove(name);
            if let Some(pos) = self.event_order.iter().position(|s| s == name) {
                self.event_order.remove(pos);
            }
        }
    }

    fn add_listener(&mut self, name: &str, callback: i64, once: bool, prepend: bool) {
        self.note_event(name);
        let vec = self.events.entry(name.to_string()).or_default();
        let listener = Listener { callback, once };
        if prepend {
            vec.insert(0, listener);
        } else {
            vec.push(listener);
        }
    }
}

static EVENTS_GC_REGISTERED: std::sync::Once = std::sync::Once::new();

fn ensure_gc_scanner_registered() {
    EVENTS_GC_REGISTERED.call_once(|| {
        gc_register_mutable_root_scanner_named("perry-ext-events", scan_events_roots);
    });
}

/// GC root scanner: visit every registered EventEmitterHandle,
/// and expose every listener closure pointer + pending Promise slot.
fn scan_events_roots(visitor: &mut GcRootVisitor<'_>) {
    iter_handles_of_mut::<EventEmitterHandle, _>(|emitter| {
        for listeners in emitter.events.values_mut() {
            for l in listeners.iter_mut() {
                if is_heap_pointer_candidate(l.callback) {
                    visitor.visit_i64_slot(&mut l.callback);
                }
            }
        }
        for pending in emitter.pending_once_promises.values_mut() {
            for p in pending.iter_mut() {
                visitor.visit_raw_mut_ptr_slot(&mut p.promise);
                visitor.visit_nanbox_f64_slot(&mut p.signal);
                if is_heap_pointer_candidate(p.abort_listener) {
                    visitor.visit_i64_slot(&mut p.abort_listener);
                }
            }
        }
    });
}

fn is_heap_pointer_candidate(callback: i64) -> bool {
    if callback <= 0 {
        return false;
    }
    let addr = callback as u64;
    (MIN_HEAP_POINTER..=MAX_HEAP_POINTER).contains(&addr) && addr & 0x7 == 0
}

unsafe fn event_target_ptr(handle: Handle) -> Option<*mut u8> {
    let addr = handle as u64;
    if !(EVENT_TARGET_MIN_HEAP_POINTER..=MAX_HEAP_POINTER).contains(&addr) || addr & 0x7 != 0 {
        return None;
    }
    let ptr = handle as *mut u8;
    if js_event_target_is_event_target(ptr as *const u8) != 0 {
        Some(ptr)
    } else {
        None
    }
}

fn handle_from_js_value_bits(bits: u64) -> Handle {
    if (bits & 0xFFFF_0000_0000_0000) == POINTER_TAG {
        (bits & POINTER_MASK) as Handle
    } else {
        bits as Handle
    }
}

unsafe fn read_str(ptr: *const StringHeader) -> Option<String> {
    let handle = JsString::from_raw(ptr as *mut StringHeader);
    read_string(handle).map(String::from)
}

fn undefined_value() -> f64 {
    f64::from_bits(TAG_UNDEFINED_F64_BITS)
}

fn object_ptr_from_value(value: f64) -> Option<*mut ObjectHeader> {
    let jsval = JsValue::from_bits(value.to_bits());
    if jsval.is_undefined() || jsval.is_null() || !jsval.is_pointer() {
        return None;
    }
    let ptr = (value.to_bits() & POINTER_MASK) as *mut ObjectHeader;
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        None
    } else {
        Some(ptr)
    }
}

unsafe fn get_object_property(value: f64, name: &[u8]) -> Option<f64> {
    let obj = object_ptr_from_value(value)?;
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, key);
    if JsValue::from_bits(value.to_bits()).is_undefined() {
        None
    } else {
        Some(value)
    }
}

unsafe fn options_signal(options: f64) -> Option<f64> {
    let jsval = JsValue::from_bits(options.to_bits());
    if jsval.is_undefined() || jsval.is_null() {
        return None;
    }
    get_object_property(options, b"signal")
        .filter(|signal| object_ptr_from_value(*signal).is_some())
}

fn signal_is_aborted(signal: f64) -> bool {
    let Some(signal_ptr) = object_ptr_from_value(signal) else {
        return false;
    };
    unsafe { js_abort_signal_is_aborted(signal_ptr as *mut u8) != 0 }
}

unsafe fn abort_event_value() -> f64 {
    let event_name = b"abort";
    let event_str = js_string_from_bytes(event_name.as_ptr(), event_name.len() as u32);
    f64::from_bits(nanbox_string_bits(event_str))
}

unsafe fn cleanup_pending_abort_listener(pending: &PendingOnce) {
    if pending.abort_listener == 0 {
        return;
    }
    let Some(signal_ptr) = object_ptr_from_value(pending.signal) else {
        return;
    };
    js_abort_signal_remove_listener(
        signal_ptr as *mut u8,
        abort_event_value(),
        nanbox_pointer_bits(pending.abort_listener),
    );
}

fn remove_pending_once_promise(
    emitter: &mut EventEmitterHandle,
    promise: *mut Promise,
) -> Option<PendingOnce> {
    let event_names: Vec<String> = emitter.pending_once_promises.keys().cloned().collect();
    for event_name in event_names {
        let mut should_prune = false;
        let removed = emitter
            .pending_once_promises
            .get_mut(&event_name)
            .and_then(|pending| {
                let pos = pending.iter().position(|p| p.promise == promise)?;
                let removed = pending.remove(pos);
                should_prune = pending.is_empty();
                Some(removed)
            });
        if should_prune {
            emitter.pending_once_promises.remove(&event_name);
        }
        if removed.is_some() {
            return removed;
        }
    }
    None
}

fn remove_listener_by_callback(emitter: &mut EventEmitterHandle, callback: i64) {
    if callback == 0 {
        return;
    }
    let event_names: Vec<String> = emitter.events.keys().cloned().collect();
    for event_name in event_names {
        let removed = if let Some(listeners) = emitter.events.get_mut(&event_name) {
            let before = listeners.len();
            listeners.retain(|listener| listener.callback != callback);
            before != listeners.len()
        } else {
            false
        };
        if removed {
            emitter.prune_event_if_empty(&event_name);
        }
    }
}

/// `new EventEmitter()` — returns a handle to the emitter.
#[no_mangle]
pub extern "C" fn js_event_emitter_new() -> Handle {
    ensure_gc_scanner_registered();
    register_handle(EventEmitterHandle::new())
}

/// `emitter.on(eventName, listener)` — register a listener.
/// Also serves as `addListener` (wired at the codegen layer).
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
/// `callback_ptr` is a raw closure pointer (the runtime's
/// `ClosureHeader` cast to i64); 0 is the no-op sentinel.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_on(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    let Some(event_name) = read_str(event_name_ptr) else {
        return handle;
    };
    if callback_ptr == 0 {
        return handle;
    }
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter.add_listener(&event_name, callback_ptr, false, false);
    }
    handle
}

/// `emitter.once(eventName, listener)` — fires once then auto-removes.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_once(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    let Some(event_name) = read_str(event_name_ptr) else {
        return handle;
    };
    if callback_ptr == 0 {
        return handle;
    }
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter.add_listener(&event_name, callback_ptr, true, false);
    }
    handle
}

/// `emitter.prependListener(eventName, listener)` — insert at front.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_prepend_listener(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    let Some(event_name) = read_str(event_name_ptr) else {
        return handle;
    };
    if callback_ptr == 0 {
        return handle;
    }
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter.add_listener(&event_name, callback_ptr, false, true);
    }
    handle
}

/// `emitter.prependOnceListener(eventName, listener)`.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_prepend_once_listener(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    let Some(event_name) = read_str(event_name_ptr) else {
        return handle;
    };
    if callback_ptr == 0 {
        return handle;
    }
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter.add_listener(&event_name, callback_ptr, true, true);
    }
    handle
}

/// Read `args_ptr[0]` or return NaN-boxed `undefined` when args is
/// null or empty. Mirrors `first_arg_or_undefined` in
/// `perry-stdlib::events` so both implementations of
/// `js_event_emitter_emit` use the same shape.
unsafe fn first_arg_or_undefined(args_ptr: *const ArrayHeader) -> f64 {
    if args_ptr.is_null() || (*args_ptr).length == 0 {
        return f64::from_bits(JsValue::UNDEFINED.bits());
    }
    f64::from_bits(js_array_get(args_ptr, 0).bits())
}

/// Drain pending `events.once` promises for `event_name` on the given
/// emitter, resolving each with the full args array passed to `emit`.
///
/// Pre-#1186 we synthesized a fresh 1-element `[arg]` array because
/// `emit` only received a single `f64` arg. Post-#1186 the codegen
/// passes the full variadic args as `*mut ArrayHeader` (`NA_VARARGS`),
/// so we resolve each Promise with that array directly — matches
/// `perry-stdlib::events::drain_pending_once_promises` and Node's
/// `events.once` semantics where the resolution value is the full args
/// tuple.
unsafe fn drain_pending_once_promises(
    emitter: &mut EventEmitterHandle,
    event_name: &str,
    args_ptr: *mut ArrayHeader,
) {
    let pending = match emitter.pending_once_promises.remove(event_name) {
        Some(v) => v,
        None => return,
    };
    let arr = if args_ptr.is_null() {
        js_array_alloc(0)
    } else {
        args_ptr
    };
    let boxed_arr = JsValue::from_object_ptr(arr);
    let bits = boxed_arr.bits();
    for pending in pending {
        cleanup_pending_abort_listener(&pending);
        if pending.promise.is_null() {
            continue;
        }
        // Synchronous resolve — see the comment on the extern at the
        // top of this file for why we bypass `JsPromise::resolve`.
        js_promise_resolve(pending.promise, f64::from_bits(bits));
    }
}

unsafe fn reject_pending_once_promises_for_error(
    emitter: &mut EventEmitterHandle,
    error_value: f64,
) -> bool {
    let event_names: Vec<String> = emitter
        .pending_once_promises
        .keys()
        .filter(|name| name.as_str() != "error")
        .cloned()
        .collect();
    let mut rejected_any = false;
    for event_name in event_names {
        let Some(pending) = emitter.pending_once_promises.remove(&event_name) else {
            continue;
        };
        for pending in pending {
            cleanup_pending_abort_listener(&pending);
            if !pending.promise.is_null() {
                js_promise_reject(pending.promise, error_value);
                rejected_any = true;
            }
        }
    }
    rejected_any
}

/// `emitter.emit(eventName, ...args)` — fire variadic `args` to every
/// listener (and to any pending `events.once` Promise for the event).
///
/// Signature must match `perry-stdlib::events::js_event_emitter_emit`
/// because `perry-codegen`'s native_table lowers `emit` calls via
/// `NA_VARARGS` (single `*mut ArrayHeader` ABI slot) and `NR_F64`
/// return. Pre-#1186 perry-ext-events still used the legacy
/// `(handle, name, arg: f64) -> bool` ABI; codegen then mis-passed the
/// args-array pointer as `arg: f64`, which the listener interpreted as
/// `NaN` because the high pointer bits set the NaN-box quiet tag —
/// see issue #1274.
///
/// Returns `TAG_TRUE` / `TAG_FALSE` (NaN-boxed booleans) so
/// `had_listeners` round-trips through codegen's f64 ABI.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_emit(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    args_ptr: *mut ArrayHeader,
) -> f64 {
    const TAG_FALSE_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0003);
    const TAG_TRUE_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0004);
    let Some(event_name) = read_str(event_name_ptr) else {
        return TAG_FALSE_F64;
    };
    let mut had_listeners = false;
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        let snapshot: Vec<Listener> = match emitter.events.get(&event_name) {
            Some(v) if !v.is_empty() => v.clone(),
            _ => Vec::new(),
        };
        if !snapshot.is_empty() {
            had_listeners = true;
            if snapshot.iter().any(|l| l.once) {
                if let Some(v) = emitter.events.get_mut(&event_name) {
                    v.retain(|l| !l.once);
                }
                emitter.prune_event_if_empty(&event_name);
            }
        }

        let first_arg = first_arg_or_undefined(args_ptr);
        if event_name == "error" {
            let has_error_once = emitter
                .pending_once_promises
                .get("error")
                .is_some_and(|pending| !pending.is_empty());
            let rejected_once = reject_pending_once_promises_for_error(emitter, first_arg);
            had_listeners = had_listeners || has_error_once || rejected_once;
            if snapshot.is_empty() && !has_error_once && !rejected_once {
                js_throw(first_arg);
            }
        }

        drain_pending_once_promises(emitter, &event_name, args_ptr);

        for l in snapshot {
            if l.callback != 0 {
                let closure = JsClosure::from_raw(l.callback as *const RawClosureHeader);
                let _ = closure.call1(first_arg);
            }
        }
    }
    if had_listeners {
        TAG_TRUE_F64
    } else {
        TAG_FALSE_F64
    }
}

/// `emitter.emit(eventName)` — no-args variant.
///
/// Same return-type contract as `js_event_emitter_emit`: returns a
/// NaN-boxed boolean (`TAG_TRUE` / `TAG_FALSE`) so callers see the
/// same shape regardless of which path they hit.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_emit0(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> f64 {
    const TAG_FALSE_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0003);
    const TAG_TRUE_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0004);
    let Some(event_name) = read_str(event_name_ptr) else {
        return TAG_FALSE_F64;
    };
    let mut had_listeners = false;
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        let snapshot: Vec<Listener> = match emitter.events.get(&event_name) {
            Some(v) if !v.is_empty() => v.clone(),
            _ => Vec::new(),
        };
        if !snapshot.is_empty() {
            had_listeners = true;
            if snapshot.iter().any(|l| l.once) {
                if let Some(v) = emitter.events.get_mut(&event_name) {
                    v.retain(|l| !l.once);
                }
                emitter.prune_event_if_empty(&event_name);
            }
        }

        let empty_args = js_array_alloc(0);
        if event_name == "error" {
            let error_value = undefined_value();
            let has_error_once = emitter
                .pending_once_promises
                .get("error")
                .is_some_and(|pending| !pending.is_empty());
            let rejected_once = reject_pending_once_promises_for_error(emitter, error_value);
            had_listeners = had_listeners || has_error_once || rejected_once;
            if snapshot.is_empty() && !has_error_once && !rejected_once {
                js_throw(error_value);
            }
        }
        drain_pending_once_promises(emitter, &event_name, empty_args);

        for l in snapshot {
            if l.callback != 0 {
                let closure = JsClosure::from_raw(l.callback as *const RawClosureHeader);
                let _ = closure.call0();
            }
        }
    }
    if had_listeners {
        TAG_TRUE_F64
    } else {
        TAG_FALSE_F64
    }
}

/// `emitter.removeListener(event, listener)`. Removes the first
/// matching listener only (matches Node).
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_remove_listener(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) -> Handle {
    let Some(event_name) = read_str(event_name_ptr) else {
        return handle;
    };
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        let mut removed = false;
        if let Some(listeners) = emitter.events.get_mut(&event_name) {
            if let Some(pos) = listeners.iter().position(|l| l.callback == callback_ptr) {
                listeners.remove(pos);
                removed = true;
            }
        }
        if removed {
            emitter.prune_event_if_empty(&event_name);
        }
    }
    handle
}

/// `emitter.removeAllListeners()` (or `(event)` to scope by event).
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_remove_all_listeners(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> Handle {
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        if event_name_ptr.is_null() {
            emitter.events.clear();
            emitter.event_order.clear();
        } else if let Some(event_name) = read_str(event_name_ptr) {
            emitter.events.remove(&event_name);
            if let Some(pos) = emitter.event_order.iter().position(|s| s == &event_name) {
                emitter.event_order.remove(pos);
            }
        }
    }
    handle
}

/// `emitter.listenerCount(eventName)`.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_listener_count(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> f64 {
    let Some(event_name) = read_str(event_name_ptr) else {
        return 0.0;
    };
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        if let Some(listeners) = emitter.events.get(&event_name) {
            return listeners.len() as f64;
        }
    }
    0.0
}

/// `emitter.setMaxListeners(n)`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_set_max_listeners(handle: Handle, n: f64) -> Handle {
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter.max_listeners = n as i32;
    }
    handle
}

/// `emitter.getMaxListeners()`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_get_max_listeners(handle: Handle) -> f64 {
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        return emitter.max_listeners as f64;
    }
    10.0
}

/// `emitter.eventNames()` — returns an array of strings in insertion
/// order (matches Node).
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_event_names(handle: Handle) -> *mut ArrayHeader {
    let arr = js_array_alloc(0);
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        let mut result = arr;
        for name in emitter.event_order.iter() {
            let alive = emitter
                .events
                .get(name)
                .map(|v| !v.is_empty())
                .unwrap_or(false);
            if !alive {
                continue;
            }
            let s = perry_ffi::alloc_string(name);
            let bits = nanbox_string_bits(s.as_raw());
            result = js_array_push(result, JsValue::from_bits(bits));
        }
        return result;
    }
    arr
}

/// `emitter.listeners(eventName)` — returns an array of the registered
/// listener closures (NaN-boxed POINTER_TAG).
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_listeners(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> *mut ArrayHeader {
    let arr = js_array_alloc(0);
    let Some(event_name) = read_str(event_name_ptr) else {
        return arr;
    };
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        if let Some(listeners) = emitter.events.get(&event_name) {
            let mut result = arr;
            for l in listeners.iter() {
                if l.callback != 0 {
                    let v = JsValue::from_object_ptr(l.callback as *mut u8);
                    result = js_array_push(result, v);
                }
            }
            return result;
        }
    }
    arr
}

/// `emitter.rawListeners(eventName)` — identical to `listeners` in our
/// model (we don't wrap once-listeners at registration time).
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_raw_listeners(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> *mut ArrayHeader {
    js_event_emitter_listeners(handle, event_name_ptr)
}

// ============================================================================
// Module-level helpers — `events.once(em, name)` / `events.on(em, name)` /
// `events.getEventListeners(em, name)` / `events.listenerCount(em, name)` /
// `events.setMaxListeners(n, em)` / `events.getMaxListeners(em)`.
// ============================================================================

extern "C" fn events_once_abort_listener(closure: *const RawClosureHeader) -> f64 {
    unsafe {
        let handle = js_closure_get_capture_ptr(closure, 0) as Handle;
        let promise = js_closure_get_capture_ptr(closure, 1) as *mut Promise;
        let pending = get_handle_mut::<EventEmitterHandle>(handle)
            .and_then(|emitter| remove_pending_once_promise(emitter, promise));
        if let Some(pending) = pending {
            cleanup_pending_abort_listener(&pending);
            if !pending.promise.is_null() {
                js_promise_reject(pending.promise, js_abort_error_value());
            }
        }
    }
    undefined_value()
}

/// `events.once(emitter, eventName[, options])` — returns a Promise that resolves
/// to a 1-element array `[arg]` on the next `emit(eventName, arg)`.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_events_once(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    options: f64,
) -> *mut Promise {
    ensure_gc_scanner_registered();
    let prom = JsPromise::new();
    let raw = prom.as_raw();
    let Some(event_name) = read_str(event_name_ptr) else {
        return raw;
    };
    let signal = options_signal(options);
    if signal.is_some_and(signal_is_aborted) {
        js_promise_reject(raw, js_abort_error_value());
        return raw;
    }
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        let mut pending = PendingOnce {
            promise: raw,
            signal: undefined_value(),
            abort_listener: 0,
        };
        if let Some(signal) = signal {
            if let Some(signal_ptr) = object_ptr_from_value(signal) {
                let abort_listener = js_closure_alloc(events_once_abort_listener as *const u8, 2);
                js_closure_set_capture_ptr(abort_listener, 0, handle);
                js_closure_set_capture_ptr(abort_listener, 1, raw as i64);
                js_abort_signal_add_listener(
                    signal_ptr as *mut u8,
                    abort_event_value(),
                    nanbox_pointer_bits(abort_listener as i64),
                );
                pending.signal = signal;
                pending.abort_listener = abort_listener as i64;
            }
        }
        emitter
            .pending_once_promises
            .entry(event_name)
            .or_default()
            .push(pending);
    }
    raw
}

/// Queue listener for `events.on(...)` — captures the queue array in
/// slot 0 and pushes `[arg]` onto it for each emitted event. The
/// `for await (... of iter)` loop pulls items off the array as the
/// stream produces them.
extern "C" fn events_on_queue_listener(closure: *const RawClosureHeader, arg0: f64) -> f64 {
    unsafe {
        let queue = js_closure_get_capture_ptr(closure, 0) as *mut ArrayHeader;
        let abort_promise = js_closure_get_capture_ptr(closure, 1) as *mut Promise;
        if !queue.is_null() {
            let mut args = js_array_alloc(0);
            args = js_array_push_f64(args, arg0);
            let args_val = nanbox_pointer_bits(args as i64);
            if abort_promise.is_null() {
                let _ = js_array_push_f64(queue, args_val);
            } else {
                let abort_val = nanbox_pointer_bits(abort_promise as i64);
                let len = (*queue).length;
                if len == 0 {
                    let _ = js_array_push_f64(queue, args_val);
                    let _ = js_array_push_f64(queue, abort_val);
                } else {
                    js_array_set(queue, len - 1, JsValue::from_bits(args_val.to_bits()));
                    let _ = js_array_push_f64(queue, abort_val);
                }
            }
        }
    }
    f64::from_bits(TAG_UNDEFINED_F64_BITS)
}

extern "C" fn events_on_abort_listener(closure: *const RawClosureHeader) -> f64 {
    unsafe {
        let handle = js_closure_get_capture_ptr(closure, 0) as Handle;
        let data_listener = js_closure_get_capture_ptr(closure, 1);
        let signal_ptr = js_closure_get_capture_ptr(closure, 2) as *mut u8;
        let abort_promise = js_closure_get_capture_ptr(closure, 3) as *mut Promise;

        if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
            remove_listener_by_callback(emitter, data_listener);
        }
        if !signal_ptr.is_null() {
            js_abort_signal_remove_listener(
                signal_ptr,
                abort_event_value(),
                nanbox_pointer_bits(closure as i64),
            );
        }
        if !abort_promise.is_null() {
            js_promise_reject(abort_promise, js_abort_error_value());
        }
    }
    undefined_value()
}

/// `events.on(emitter, eventName)` — returns an async-iterable queue of
/// argument arrays. Perry's `for await` lowering already accepts plain arrays
/// as async-iterable inputs, so the implementation backs the iterator with an
/// Array and appends one `[arg]` entry per emitted event. Ported from
/// `perry-stdlib/src/events.rs` (#1557).
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_events_on(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    options: f64,
) -> *mut ArrayHeader {
    ensure_gc_scanner_registered();
    let queue = js_array_alloc(0);
    let Some(event_name) = read_str(event_name_ptr) else {
        return queue;
    };
    let signal = options_signal(options);
    if signal.is_some_and(signal_is_aborted) {
        js_throw(js_abort_error_value());
    }
    let abort_promise = if signal.is_some() {
        JsPromise::new().as_raw()
    } else {
        std::ptr::null_mut()
    };

    let listener = js_closure_alloc(events_on_queue_listener as *const u8, 2);
    js_closure_set_capture_ptr(listener, 0, queue as i64);
    js_closure_set_capture_ptr(listener, 1, abort_promise as i64);
    if !abort_promise.is_null() {
        let _ = js_array_push_f64(queue, nanbox_pointer_bits(abort_promise as i64));
    }

    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter.add_listener(&event_name, listener as i64, false, false);
        if let Some(signal) = signal {
            if let Some(signal_ptr) = object_ptr_from_value(signal) {
                let abort_listener = js_closure_alloc(events_on_abort_listener as *const u8, 4);
                js_closure_set_capture_ptr(abort_listener, 0, handle);
                js_closure_set_capture_ptr(abort_listener, 1, listener as i64);
                js_closure_set_capture_ptr(abort_listener, 2, signal_ptr as i64);
                js_closure_set_capture_ptr(abort_listener, 3, abort_promise as i64);
                js_abort_signal_add_listener(
                    signal_ptr as *mut u8,
                    abort_event_value(),
                    nanbox_pointer_bits(abort_listener as i64),
                );
            }
        }
    }
    queue
}

/// `events.addAbortListener(signal, listener)` — attach `listener` to the
/// AbortSignal's "abort" event and return a `Disposable`-shaped plain object
/// (currently a function-shaped placeholder — listener removal can be
/// tightened later). Ported from `perry-stdlib/src/events.rs` (#1557).
///
/// # Safety
///
/// `signal_ptr` must be null or a Perry-runtime `ObjectHeader` (the
/// AbortSignal instance); `callback_ptr` must be null or a closure pointer.
#[no_mangle]
pub unsafe extern "C" fn js_events_add_abort_listener(signal_ptr: i64, callback_ptr: i64) -> i64 {
    if signal_ptr == 0 || callback_ptr == 0 {
        return 0;
    }
    let event_name = b"abort";
    let event_str = js_string_from_bytes(event_name.as_ptr(), event_name.len() as u32);
    let event_val = f64::from_bits(nanbox_string_bits(event_str));
    let listener_val = nanbox_pointer_bits(callback_ptr);
    js_abort_signal_add_listener(signal_ptr as *mut u8, event_val, listener_val);
    // Perry currently surfaces the disposable as the listener itself
    // (matching node's `{ [Symbol.dispose]: () => signal.removeEventListener(...) }`
    // shape requires symbol-property writes that this crate doesn't expose
    // yet; perry-stdlib's version uses perry_runtime::symbol helpers). The
    // returned pointer is callable, so `disposable[Symbol.dispose]?.()` won't
    // crash — it just won't actually unsubscribe. Tightening this is tracked
    // in #1557's follow-up bullet.
    callback_ptr
}

/// `events.getEventListeners(emitter, eventName)` — alias for
/// `emitter.listeners(eventName)`.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_events_get_event_listeners(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> *mut ArrayHeader {
    if get_handle_mut::<EventEmitterHandle>(handle).is_some() {
        return js_event_emitter_listeners(handle, event_name_ptr);
    }
    if let Some(target) = event_target_ptr(handle) {
        return js_event_target_get_event_listeners(target, event_name_ptr);
    }
    js_array_alloc(0)
}

/// `events.listenerCount(emitter, eventName)` — alias.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_events_listener_count(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> f64 {
    js_event_emitter_listener_count(handle, event_name_ptr)
}

/// `events.getMaxListeners(emitter)` — alias.
#[no_mangle]
pub unsafe extern "C" fn js_events_get_max_listeners(handle: Handle) -> f64 {
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        return emitter.max_listeners as f64;
    }
    if let Some(target) = event_target_ptr(handle) {
        return js_event_target_get_max_listeners(target);
    }
    10.0
}

/// `events.setMaxListeners(n, ...emitters)` — Perry FFI takes a single
/// array of target handles from the codegen varargs lowering.
#[no_mangle]
pub unsafe extern "C" fn js_events_set_max_listeners(
    n: f64,
    handles_ptr: *const ArrayHeader,
) -> f64 {
    if !handles_ptr.is_null() {
        let len = (*handles_ptr).length;
        for i in 0..len {
            let handle = handle_from_js_value_bits(js_array_get(handles_ptr, i).bits());
            if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
                emitter.max_listeners = n as i32;
            } else if let Some(target) = event_target_ptr(handle) {
                let _ = js_event_target_set_max_listeners(target, n);
            }
        }
    }
    f64::from_bits(0x7FFC_0000_0000_0001)
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_ffi::{alloc_string, drop_handle, get_handle, register_handle};
    use std::collections::HashMap;
    use std::sync::{Mutex, MutexGuard};

    static GC_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct GcTestGuard {
        frame: u64,
        _lock: MutexGuard<'static, ()>,
    }

    impl GcTestGuard {
        fn new() -> Self {
            let lock = GC_TEST_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            perry_runtime::gc::js_gc_write_barriers_emitted(1);
            let frame = perry_runtime::gc::js_shadow_frame_push(0);
            Self { frame, _lock: lock }
        }
    }

    impl Drop for GcTestGuard {
        fn drop(&mut self) {
            perry_runtime::gc::js_shadow_frame_pop(self.frame);
            perry_runtime::gc::js_gc_write_barriers_emitted(0);
        }
    }

    fn young_gc_root() -> i64 {
        perry_runtime::arena::arena_alloc_gc(32, 8, perry_runtime::gc::GC_TYPE_STRING) as i64
    }

    fn young_promise_root() -> *mut Promise {
        let ptr = perry_runtime::arena::arena_alloc_gc(
            std::mem::size_of::<perry_runtime::Promise>(),
            std::mem::align_of::<perry_runtime::Promise>(),
            perry_runtime::gc::GC_TYPE_PROMISE,
        );
        unsafe {
            std::ptr::write_bytes(ptr, 0, std::mem::size_of::<perry_runtime::Promise>());
        }
        ptr as *mut Promise
    }

    fn assert_rewritten(before: usize, after: usize) {
        assert_ne!(after, before);
        assert!(perry_runtime::arena::pointer_in_nursery(after));
    }

    #[test]
    fn new_emitter_starts_empty() {
        let h = js_event_emitter_new();
        let event_name = alloc_string("foo");
        let count = unsafe { js_event_emitter_listener_count(h, event_name.as_raw() as *const _) };
        assert_eq!(count, 0.0);
        drop_handle(h);
    }

    #[test]
    fn add_then_count_listeners() {
        let h = js_event_emitter_new();
        let event_name = alloc_string("change");
        // Non-zero sentinel — we never emit so the closures aren't invoked.
        let _ = unsafe { js_event_emitter_on(h, event_name.as_raw() as *const _, 0xDEADBEEF_i64) };
        let _ = unsafe { js_event_emitter_on(h, event_name.as_raw() as *const _, 0xCAFEBABE_i64) };
        let count = unsafe { js_event_emitter_listener_count(h, event_name.as_raw() as *const _) };
        assert_eq!(count, 2.0);
        drop_handle(h);
    }

    #[test]
    fn remove_listener_drops_one() {
        let h = js_event_emitter_new();
        let event_name = alloc_string("data");
        unsafe {
            js_event_emitter_on(h, event_name.as_raw() as *const _, 1);
            js_event_emitter_on(h, event_name.as_raw() as *const _, 2);
            js_event_emitter_remove_listener(h, event_name.as_raw() as *const _, 1);
        }
        let count = unsafe { js_event_emitter_listener_count(h, event_name.as_raw() as *const _) };
        assert_eq!(count, 1.0);
        drop_handle(h);
    }

    #[test]
    fn remove_all_clears() {
        let h = js_event_emitter_new();
        let event_name = alloc_string("x");
        unsafe {
            js_event_emitter_on(h, event_name.as_raw() as *const _, 1);
            js_event_emitter_on(h, event_name.as_raw() as *const _, 2);
            js_event_emitter_remove_all_listeners(h, std::ptr::null());
        }
        let count = unsafe { js_event_emitter_listener_count(h, event_name.as_raw() as *const _) };
        assert_eq!(count, 0.0);
        drop_handle(h);
    }

    #[test]
    fn prepend_listener_inserts_at_front() {
        let h = js_event_emitter_new();
        let event_name = alloc_string("ord");
        unsafe {
            js_event_emitter_on(h, event_name.as_raw() as *const _, 100);
            js_event_emitter_prepend_listener(h, event_name.as_raw() as *const _, 99);
        }
        let arr = unsafe { js_event_emitter_listeners(h, event_name.as_raw() as *const _) };
        assert!(!arr.is_null());
        drop_handle(h);
    }

    #[test]
    fn max_listeners_round_trips() {
        let h = js_event_emitter_new();
        // Default = 10.
        assert_eq!(unsafe { js_event_emitter_get_max_listeners(h) }, 10.0);
        unsafe {
            js_event_emitter_set_max_listeners(h, 42.0);
        }
        assert_eq!(unsafe { js_event_emitter_get_max_listeners(h) }, 42.0);
        drop_handle(h);
    }

    #[test]
    fn gc_mutable_scanner_rewrites_listener_and_pending_promise_roots() {
        let _guard = GcTestGuard::new();
        perry_ffi::gc_register_mutable_root_scanner_named("perry-ext-events", scan_events_roots);

        let listener = young_gc_root();
        let promise = young_promise_root();
        let mut events = HashMap::new();
        events.insert(
            "ready".to_string(),
            vec![Listener {
                callback: listener,
                once: false,
            }],
        );
        let mut pending_once_promises = HashMap::new();
        pending_once_promises.insert(
            "ready".to_string(),
            vec![PendingOnce {
                promise,
                signal: undefined_value(),
                abort_listener: 0,
            }],
        );
        let handle = register_handle(EventEmitterHandle {
            events,
            event_order: vec!["ready".to_string()],
            pending_once_promises,
            max_listeners: 10,
        });

        let _ = perry_runtime::gc::gc_collect_minor();

        {
            let emitter = get_handle::<EventEmitterHandle>(handle)
                .expect("emitter handle should remain live");
            assert_rewritten(
                listener as usize,
                emitter.events["ready"][0].callback as usize,
            );
            assert_rewritten(
                promise as usize,
                emitter.pending_once_promises["ready"][0].promise as usize,
            );
        }
        drop_handle(handle);
    }
}
