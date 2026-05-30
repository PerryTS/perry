//! EventEmitter implementation
//!
//! Native implementation of Node.js EventEmitter pattern.
//! Rewritten for issue #850 — Node-compatible listener-table semantics
//! covering `on` / `once` / `addListener` / `prependListener` /
//! `prependOnceListener` / `removeListener` / `removeAllListeners` /
//! `listenerCount` / `listeners` / `rawListeners` / `eventNames` /
//! `setMaxListeners` / `getMaxListeners`, plus the module-level
//! `events.once` / `events.getEventListeners` / `events.listenerCount` /
//! `events.setMaxListeners` / `events.getMaxListeners` helpers.
//!
//! ## Storage model
//!
//! Each `EventEmitterHandle` stores an ordered list of events (so
//! `eventNames()` returns insertion order, matching Node) plus per-event
//! `Vec<Listener>` with insert-back (`on`/`addListener`) and insert-front
//! (`prependListener`). Each `Listener` carries a `once` flag — `emit`
//! collects all `once` listeners, fires the whole snapshot, then prunes
//! the fired ones from the live list. Pending `events.once` promises are
//! stored alongside listeners so a single `emit` can resolve them all.

use perry_runtime::{
    js_array_alloc, js_array_length, js_array_push_f64, js_closure_call0, js_closure_call1,
    js_closure_call2, js_nanbox_get_pointer, js_nanbox_pointer, js_nanbox_string, js_object_alloc,
    js_object_get_field_by_name, js_object_get_field_by_name_f64, js_promise_new,
    js_promise_reject, js_promise_resolve, js_string_from_bytes, ArrayHeader, ClosureHeader,
    JSValue, ObjectHeader, Promise, StringHeader,
};
use std::collections::HashMap;

const TAG_FALSE_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0003);
const TAG_TRUE_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0004);
const TAG_NULL_F64_BITS: u64 = 0x7FFC_0000_0000_0002;
const ERROR_MONITOR_EVENT_NAME: &str = "Symbol(events.errorMonitor)";
const MIN_HEAP_POINTER: u64 = 0x10000;
const MAX_HEAP_POINTER: u64 = 0x0000_FFFF_FFFF_FFFF;

fn bool_to_js(value: bool) -> f64 {
    if value {
        TAG_TRUE_F64
    } else {
        TAG_FALSE_F64
    }
}

unsafe fn event_target_ptr(handle: Handle) -> Option<*mut ObjectHeader> {
    let addr = handle as u64;
    if !(MIN_HEAP_POINTER..=MAX_HEAP_POINTER).contains(&addr) || addr & 0x7 != 0 {
        return None;
    }
    let ptr = handle as *mut ObjectHeader;
    if perry_runtime::event_target::js_event_target_is_event_target(ptr) != 0 {
        Some(ptr)
    } else {
        None
    }
}

unsafe fn stream_listeners_for_heap_object(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> Option<*mut ArrayHeader> {
    let addr = handle as u64;
    if event_name_ptr.is_null()
        || !(MIN_HEAP_POINTER..=MAX_HEAP_POINTER).contains(&addr)
        || addr & 0x7 != 0
    {
        return None;
    }
    let event = js_nanbox_string(event_name_ptr as i64);
    Some(
        perry_runtime::node_stream::js_node_stream_method_listeners(handle, event)
            as *mut ArrayHeader,
    )
}

fn throw_max_listeners_out_of_range() -> ! {
    static REGISTER_RANGE_ERROR: std::sync::Once = std::sync::Once::new();
    REGISTER_RANGE_ERROR.call_once(|| {
        perry_runtime::object::js_register_class_extends_error(
            perry_runtime::error::CLASS_ID_RANGE_ERROR,
        );
    });

    let obj = js_object_alloc(perry_runtime::error::CLASS_ID_RANGE_ERROR, 4);
    let string_value = |bytes: &[u8]| -> f64 {
        let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        js_nanbox_string(ptr as i64)
    };
    let set = |key: &[u8], value: f64| {
        let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
        perry_runtime::js_object_set_field_by_name(obj, key_ptr, value);
    };
    set(b"name", string_value(b"RangeError"));
    set(b"code", string_value(b"ERR_OUT_OF_RANGE"));
    set(b"message", string_value(b"The value is out of range"));
    perry_runtime::exception::js_throw(js_nanbox_pointer(obj as i64))
}

#[inline]
fn validate_max_listeners(n: f64) {
    if n.is_nan() || n < 0.0 {
        throw_max_listeners_out_of_range();
    }
}

use crate::common::{for_each_handle_mut_of, get_handle, get_handle_mut, register_handle, Handle};

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

/// EventEmitter handle.
///
/// `events` is a `HashMap<String, Vec<Listener>>` for O(1) lookup; the
/// parallel `event_order` `Vec<String>` preserves insertion order so
/// `eventNames()` matches Node's behaviour (first-seen order).
pub struct EventEmitterHandle {
    /// Event name → list of listeners. Order within the Vec is dispatch
    /// order (front-of-Vec fires first).
    events: HashMap<String, Vec<Listener>>,
    /// Insertion-order shadow of `events.keys()`. Names that get fully
    /// drained (e.g. via `removeAllListeners(name)`) are removed.
    event_order: Vec<String>,
    /// Per-event pending `events.once(em, name)` promises. Resolved on
    /// the next `emit(name, ...)` with the emitted args array.
    pending_once_promises: HashMap<String, Vec<PendingOnce>>,
    /// `setMaxListeners` ceiling. Node's default is 10 but we don't warn
    /// when the count exceeds it — `getMaxListeners()` just reads back
    /// whatever was written.
    max_listeners: f64,
    /// Constructor-level `{ captureRejections: true }` flag. When enabled,
    /// rejected promises returned from listeners are routed to `"error"`.
    capture_rejections: bool,
}

// SAFETY: pending records hold raw GC-managed pointers, but the
// registry's GC scanner visits each slot so copied-minor collection can
// keep them live and rewrite moved addresses.
unsafe impl Send for EventEmitterHandle {}
unsafe impl Sync for EventEmitterHandle {}

static EVENTS_GC_REGISTERED: std::sync::Once = std::sync::Once::new();

/// Register the EventEmitter GC root scanner exactly once. User closures
/// passed to `emitter.on(event, cb)` live inside EventEmitterHandle
/// values in the handle registry; without this scanner, a malloc-triggered
/// GC between `.on(...)` and the next `.emit(...)` would sweep the
/// closure — same root cause as issue #35 for net.Socket listeners.
fn ensure_gc_scanner_registered() {
    EVENTS_GC_REGISTERED.call_once(|| {
        perry_runtime::gc::gc_register_mutable_root_scanner_named(
            "stdlib:events",
            scan_events_roots_mut,
        );
    });
}

/// GC root scanner for EventEmitter listener closures and pending
/// `events.once` promises.
#[allow(dead_code)]
fn scan_events_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = perry_runtime::gc::RuntimeRootVisitor::for_copy(mark);
    scan_events_roots_mut(&mut visitor);
}

fn scan_events_roots_mut(visitor: &mut perry_runtime::gc::RuntimeRootVisitor<'_>) {
    for_each_handle_mut_of::<EventEmitterHandle, _>(|emitter| {
        for listeners in emitter.events.values_mut() {
            for l in listeners.iter_mut() {
                visitor.visit_i64_slot(&mut l.callback);
            }
        }
        for pending in emitter.pending_once_promises.values_mut() {
            for p in pending.iter_mut() {
                visitor.visit_raw_mut_ptr_slot(&mut p.promise);
                visitor.visit_nanbox_f64_slot(&mut p.signal);
                if p.abort_listener != 0 {
                    visitor.visit_i64_slot(&mut p.abort_listener);
                }
            }
        }
    });
}

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
            // Node's default is 10. We mirror it so `getMaxListeners()`
            // on a fresh emitter returns 10 (matching Node).
            max_listeners: 10.0,
            capture_rejections: false,
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

    fn emit_meta_event(&self, meta_name: &str, event_name: &str, listener_arg: i64) {
        let snapshot = match self.events.get(meta_name) {
            Some(v) if !v.is_empty() => v.clone(),
            _ => return,
        };
        let bytes = event_name.as_bytes();
        let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        let event_arg = js_nanbox_string(str_ptr as i64);
        let listener_arg = js_nanbox_pointer(listener_arg);
        for l in snapshot {
            if l.callback != 0 {
                let closure_ptr = l.callback as *const ClosureHeader;
                js_closure_call2(closure_ptr, event_arg, listener_arg);
            }
        }
    }

    fn add_listener(&mut self, name: &str, callback: i64, once: bool, prepend: bool) {
        self.emit_meta_event("newListener", name, callback);
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

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }

    let sym_ptr = ptr as *const perry_runtime::symbol::SymbolHeader;
    if (*sym_ptr).magic == perry_runtime::symbol::SYMBOL_MAGIC {
        let sym_value = js_nanbox_pointer(ptr as i64);
        let rendered = perry_runtime::symbol::js_symbol_to_string(sym_value);
        return string_from_header(rendered as *const StringHeader);
    }

    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
}

fn undefined_value() -> f64 {
    f64::from_bits(TAG_UNDEFINED_F64_BITS)
}

fn object_ptr_from_value(value: f64) -> Option<*mut ObjectHeader> {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_undefined() || jsval.is_null() || !jsval.is_pointer() {
        return None;
    }
    let ptr = js_nanbox_get_pointer(value) as *mut ObjectHeader;
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        None
    } else {
        Some(ptr)
    }
}

fn stream_value_from_handle(handle: Handle) -> Option<f64> {
    let addr = handle as u64;
    if !(MIN_HEAP_POINTER..=MAX_HEAP_POINTER).contains(&addr) || addr & 0x7 != 0 {
        return None;
    }
    let value = js_nanbox_pointer(handle);
    let readable = perry_runtime::node_stream::js_node_stream_is_readable(value);
    let writable = perry_runtime::node_stream::js_node_stream_is_writable(value);
    if readable.to_bits() == TAG_NULL_F64_BITS && writable.to_bits() == TAG_NULL_F64_BITS {
        None
    } else {
        Some(value)
    }
}

unsafe fn get_object_property(value: f64, name: &[u8]) -> Option<f64> {
    let obj = object_ptr_from_value(value)?;
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, key);
    if JSValue::from_bits(value.to_bits()).is_undefined() {
        None
    } else {
        Some(value)
    }
}

unsafe fn options_signal(options: f64) -> Option<f64> {
    let jsval = JSValue::from_bits(options.to_bits());
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
    perry_runtime::url::js_abort_signal_is_aborted(signal_ptr) != 0
}

unsafe fn abort_event_value() -> f64 {
    let event_name = b"abort";
    let event_str = js_string_from_bytes(event_name.as_ptr(), event_name.len() as u32);
    js_nanbox_string(event_str as i64)
}

unsafe fn cleanup_pending_abort_listener(pending: &PendingOnce) {
    if pending.abort_listener == 0 {
        return;
    }
    let Some(signal_ptr) = object_ptr_from_value(pending.signal) else {
        return;
    };
    let listener_val = js_nanbox_pointer(pending.abort_listener);
    perry_runtime::url::js_abort_signal_remove_listener(
        signal_ptr,
        abort_event_value(),
        listener_val,
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

unsafe fn dispatch_error_monitor(emitter: &mut EventEmitterHandle, arg: Option<f64>) {
    let snapshot: Vec<Listener> = match emitter.events.get(ERROR_MONITOR_EVENT_NAME) {
        Some(v) if !v.is_empty() => v.clone(),
        _ => return,
    };
    if snapshot.iter().any(|l| l.once) {
        if let Some(v) = emitter.events.get_mut(ERROR_MONITOR_EVENT_NAME) {
            v.retain(|l| !l.once);
        }
        emitter.prune_event_if_empty(ERROR_MONITOR_EVENT_NAME);
    }

    for l in snapshot {
        if l.callback != 0 {
            let closure_ptr = l.callback as *const ClosureHeader;
            if let Some(arg) = arg {
                js_closure_call1(closure_ptr, arg);
            } else {
                js_closure_call0(closure_ptr);
            }
        }
    }
}

unsafe fn event_emitter_options_capture_rejections(options: f64) -> bool {
    if !JSValue::from_bits(options.to_bits()).is_pointer() {
        return false;
    }
    let options_obj = js_nanbox_get_pointer(options) as *const ObjectHeader;
    if options_obj.is_null() || (options_obj as usize) < 0x100000 {
        return false;
    }
    let gc_header = (options_obj as *const u8).sub(perry_runtime::gc::GC_HEADER_SIZE)
        as *const perry_runtime::gc::GcHeader;
    if (*gc_header).obj_type != perry_runtime::gc::GC_TYPE_OBJECT {
        return false;
    }
    let key = b"captureRejections";
    let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let value = js_object_get_field_by_name(options_obj, key_ptr);
    perry_runtime::value::js_is_truthy(f64::from_bits(value.bits())) != 0
}

/// Create a new EventEmitter
/// Returns a handle (i64) to the emitter
#[no_mangle]
pub extern "C" fn js_event_emitter_new() -> Handle {
    ensure_gc_scanner_registered();
    register_handle(EventEmitterHandle::new())
}

#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_new_with_options(options: f64) -> Handle {
    ensure_gc_scanner_registered();
    let mut emitter = EventEmitterHandle::new();
    emitter.capture_rejections = event_emitter_options_capture_rejections(options);
    register_handle(emitter)
}

pub fn is_event_emitter_handle(handle: Handle) -> bool {
    get_handle::<EventEmitterHandle>(handle).is_some()
}

/// EventEmitter.on(eventName, listener) — also serves as `addListener`.
/// Register a listener for the specified event.
/// Returns the emitter handle for chaining.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_on(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => return handle,
    };
    if callback_ptr == 0 {
        return handle;
    }
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter.add_listener(&event_name, callback_ptr, false, false);
    }
    handle
}

/// EventEmitter.once(eventName, listener) — fires once, then auto-removes.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_once(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => return handle,
    };
    if callback_ptr == 0 {
        return handle;
    }
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter.add_listener(&event_name, callback_ptr, true, false);
    }
    handle
}

/// EventEmitter.prependListener(eventName, listener) — insert at front.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_prepend_listener(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => return handle,
    };
    if callback_ptr == 0 {
        return handle;
    }
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter.add_listener(&event_name, callback_ptr, false, true);
    }
    handle
}

/// EventEmitter.prependOnceListener(eventName, listener).
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_prepend_once_listener(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => return handle,
    };
    if callback_ptr == 0 {
        return handle;
    }
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter.add_listener(&event_name, callback_ptr, true, true);
    }
    handle
}

/// Drain pending `events.once` promises for `event_name` on `handle`,
/// resolving each with the full emitted args array.
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
    let boxed_arr = js_nanbox_pointer(arr as i64);
    for pending in pending {
        cleanup_pending_abort_listener(&pending);
        if !pending.promise.is_null() {
            js_promise_resolve(pending.promise, boxed_arr);
        }
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

unsafe fn first_arg_or_undefined(args_ptr: *const ArrayHeader) -> f64 {
    if args_ptr.is_null() || js_array_length(args_ptr) == 0 {
        f64::from_bits(TAG_UNDEFINED_F64_BITS)
    } else {
        perry_runtime::array::js_array_get_f64(args_ptr, 0)
    }
}

unsafe fn collect_emit_args(args_ptr: *const ArrayHeader) -> Vec<f64> {
    if args_ptr.is_null() {
        return Vec::new();
    }

    let len = js_array_length(args_ptr) as usize;
    let mut args = Vec::with_capacity(len);
    for index in 0..len {
        args.push(perry_runtime::array::js_array_get_f64(
            args_ptr,
            index as u32,
        ));
    }
    args
}

unsafe fn call_emitter_listener(handle: Handle, callback: i64, args: &[f64]) -> f64 {
    let receiver = js_nanbox_pointer(handle);
    let callback_value = js_nanbox_pointer(callback);
    let previous_this = perry_runtime::object::js_implicit_this_set(receiver);
    let result =
        perry_runtime::closure::js_native_call_value(callback_value, args.as_ptr(), args.len());
    perry_runtime::object::js_implicit_this_set(previous_this);
    result
}

const TAG_UNDEFINED_F64_BITS: u64 = 0x7FFC_0000_0000_0001;

extern "C" fn events_capture_rejection_handler(closure: *const ClosureHeader, reason: f64) -> f64 {
    use perry_runtime::closure::js_closure_get_capture_ptr;

    let handle = js_closure_get_capture_ptr(closure, 0) as Handle;
    if handle != 0 {
        let event_name = b"error";
        let event_str = js_string_from_bytes(event_name.as_ptr(), event_name.len() as u32);
        let mut args = js_array_alloc(0);
        args = js_array_push_f64(args, reason);
        unsafe {
            js_event_emitter_emit(handle, event_str, args);
        }
    }
    f64::from_bits(TAG_UNDEFINED_F64_BITS)
}

unsafe fn capture_listener_rejection(handle: Handle, result: f64) {
    use perry_runtime::closure::{js_closure_alloc, js_closure_set_capture_ptr};

    if perry_runtime::promise::js_value_is_promise(result) == 0 {
        return;
    }
    let promise = js_nanbox_get_pointer(result) as *mut Promise;
    if promise.is_null() {
        return;
    }
    let on_rejected = js_closure_alloc(events_capture_rejection_handler as *const u8, 1);
    js_closure_set_capture_ptr(on_rejected, 0, handle);
    perry_runtime::promise::js_promise_then(promise, std::ptr::null(), on_rejected);
}

/// EventEmitter.emit(eventName, ...args)
/// Emit an event with variadic arguments packed into an ArrayHeader.
/// Returns true if there were listeners, false otherwise.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_emit(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    args_ptr: *mut ArrayHeader,
) -> f64 {
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => return TAG_FALSE_F64,
    };

    let mut had_listeners = false;
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        // Snapshot the listener vec, then prune `once`-listeners from
        // the live vec before dispatching. This matches Node semantics:
        // a once-listener removed mid-dispatch still fires this emit,
        // but is gone for the next one.
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
        let emitted_args = collect_emit_args(args_ptr);
        if event_name == "error" {
            dispatch_error_monitor(emitter, Some(first_arg));
            let has_error_once = emitter
                .pending_once_promises
                .get("error")
                .is_some_and(|pending| !pending.is_empty());
            let rejected_once = reject_pending_once_promises_for_error(emitter, first_arg);
            had_listeners = had_listeners || has_error_once || rejected_once;
            if snapshot.is_empty() && !has_error_once && !rejected_once {
                perry_runtime::exception::js_throw(first_arg);
            }
        }

        // Resolve any pending `events.once` Promises before dispatch.
        drain_pending_once_promises(emitter, &event_name, args_ptr);

        let capture_rejections = emitter.capture_rejections && event_name != "error";
        for l in snapshot {
            if l.callback != 0 {
                let result = call_emitter_listener(handle, l.callback, &emitted_args);
                if capture_rejections {
                    capture_listener_rejection(handle, result);
                }
            }
        }
    }

    bool_to_js(had_listeners)
}

/// EventEmitter.emit with no arguments
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_emit0(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> f64 {
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => return TAG_FALSE_F64,
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
        let emitted_args: &[f64] = &[];
        if event_name == "error" {
            dispatch_error_monitor(emitter, None);
            let has_error_once = emitter
                .pending_once_promises
                .get("error")
                .is_some_and(|pending| !pending.is_empty());
            let rejected_once = reject_pending_once_promises_for_error(
                emitter,
                f64::from_bits(TAG_UNDEFINED_F64_BITS),
            );
            had_listeners = had_listeners || has_error_once || rejected_once;
            if snapshot.is_empty() && !has_error_once && !rejected_once {
                perry_runtime::exception::js_throw(f64::from_bits(TAG_UNDEFINED_F64_BITS));
            }
        }
        drain_pending_once_promises(emitter, &event_name, empty_args);

        let capture_rejections = emitter.capture_rejections && event_name != "error";
        for l in snapshot {
            if l.callback != 0 {
                let result = call_emitter_listener(handle, l.callback, emitted_args);
                if capture_rejections {
                    capture_listener_rejection(handle, result);
                }
            }
        }
    }

    bool_to_js(had_listeners)
}

/// EventEmitter.removeListener(eventName, listener)
/// Remove the most recently added matching listener for the event.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_remove_listener(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) -> Handle {
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => return handle,
    };

    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        let mut removed = false;
        if let Some(listeners) = emitter.events.get_mut(&event_name) {
            // Node removes only one matching listener: the most recently added
            // instance among duplicates.
            if let Some(pos) = listeners.iter().rposition(|l| l.callback == callback_ptr) {
                listeners.remove(pos);
                removed = true;
            }
        }
        if removed {
            emitter.prune_event_if_empty(&event_name);
            emitter.emit_meta_event("removeListener", &event_name, callback_ptr);
        }
    }
    handle
}

/// EventEmitter.removeAllListeners(eventName?)
/// Remove all listeners for an event (or all events if no name given).
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_remove_all_listeners(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> Handle {
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        if event_name_ptr.is_null() {
            let removed: Vec<(String, i64)> = emitter
                .event_order
                .iter()
                .filter(|name| name.as_str() != "removeListener")
                .flat_map(|name| {
                    emitter.events.get(name).into_iter().flat_map(|listeners| {
                        listeners
                            .iter()
                            .map(|listener| (name.clone(), listener.callback))
                    })
                })
                .collect();
            emitter.events.clear();
            emitter.event_order.clear();
            for (name, callback) in removed {
                emitter.emit_meta_event("removeListener", &name, callback);
            }
        } else if let Some(event_name) = string_from_header(event_name_ptr) {
            let removed: Vec<i64> = emitter
                .events
                .get(&event_name)
                .map(|listeners| listeners.iter().map(|listener| listener.callback).collect())
                .unwrap_or_default();
            emitter.events.remove(&event_name);
            if let Some(pos) = emitter.event_order.iter().position(|s| s == &event_name) {
                emitter.event_order.remove(pos);
            }
            if event_name != "removeListener" {
                for callback in removed {
                    emitter.emit_meta_event("removeListener", &event_name, callback);
                }
            }
        }
    }
    handle
}

/// EventEmitter.listenerCount(eventName)
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_listener_count(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) -> f64 {
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => return 0.0,
    };
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        if let Some(listeners) = emitter.events.get(&event_name) {
            if callback_ptr != 0 {
                return listeners
                    .iter()
                    .filter(|listener| listener.callback == callback_ptr)
                    .count() as f64;
            }
            return listeners.len() as f64;
        }
    }
    0.0
}

/// EventEmitter.setMaxListeners(n).
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_set_max_listeners(handle: Handle, n: f64) -> Handle {
    validate_max_listeners(n);
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter.max_listeners = n;
    }
    handle
}

/// EventEmitter.getMaxListeners().
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_get_max_listeners(handle: Handle) -> f64 {
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        return emitter.max_listeners;
    }
    // Node's default for a stranger emitter is 10.
    10.0
}

/// EventEmitter.eventNames() — returns an array of strings in insertion
/// order (matches Node).
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_event_names(handle: Handle) -> *mut ArrayHeader {
    let arr = js_array_alloc(0);
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        let mut result = arr;
        for name in emitter.event_order.iter() {
            // Skip events that have been emptied without prune (shouldn't
            // happen, but defensive).
            let alive = emitter
                .events
                .get(name)
                .map(|v| !v.is_empty())
                .unwrap_or(false);
            if !alive {
                continue;
            }
            let bytes = name.as_bytes();
            let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            let nanboxed = js_nanbox_string(str_ptr as i64);
            result = js_array_push_f64(result, nanboxed);
        }
        return result;
    }
    arr
}

/// EventEmitter.listeners(eventName) — returns an array of the registered
/// listener closures (NaN-boxed POINTER_TAG). For the `once` case Node
/// returns the *unwrapped* user closure; we already store the user
/// closure directly so the result matches.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_listeners(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> *mut ArrayHeader {
    let arr = js_array_alloc(0);
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => return arr,
    };
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        if let Some(listeners) = emitter.events.get(&event_name) {
            let mut result = arr;
            for l in listeners.iter() {
                if l.callback != 0 {
                    let nanboxed = js_nanbox_pointer(l.callback);
                    result = js_array_push_f64(result, nanboxed);
                }
            }
            return result;
        }
    }
    arr
}

/// EventEmitter.rawListeners(eventName) — identical to `listeners` in
/// our model since we don't wrap `once` listeners at registration time
/// (the `once` flag is stored alongside the user closure).
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_raw_listeners(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> *mut ArrayHeader {
    js_event_emitter_listeners(handle, event_name_ptr)
}

// ============================================================================
// Module-level helpers — `events.once(em, name)`, `events.on(em, name)`,
// `events.getEventListeners(em, name)`, `events.listenerCount(em, name)`,
// `events.setMaxListeners(n, em)`, `events.getMaxListeners(em)`.
// ============================================================================

extern "C" fn events_once_abort_listener(closure: *const ClosureHeader) -> f64 {
    use perry_runtime::closure::js_closure_get_capture_ptr;

    let handle = js_closure_get_capture_ptr(closure, 0) as Handle;
    let promise = js_closure_get_capture_ptr(closure, 1) as *mut Promise;

    let pending = get_handle_mut::<EventEmitterHandle>(handle)
        .and_then(|emitter| remove_pending_once_promise(emitter, promise));
    if let Some(pending) = pending {
        unsafe {
            cleanup_pending_abort_listener(&pending);
            if !pending.promise.is_null() {
                js_promise_reject(pending.promise, perry_runtime::url::js_abort_error_value());
            }
        }
    }

    undefined_value()
}

extern "C" fn events_once_stream_resolve_listener(closure: *const ClosureHeader, rest: f64) -> f64 {
    use perry_runtime::closure::js_closure_get_capture_ptr;

    let promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let handle = js_closure_get_capture_ptr(closure, 1) as Handle;
    let error_listener = js_closure_get_capture_ptr(closure, 2);
    let error_event_ptr = js_closure_get_capture_ptr(closure, 3);
    if promise.is_null() {
        return undefined_value();
    }
    if handle != 0 && error_listener != 0 && error_event_ptr != 0 {
        let error_event = js_nanbox_string(error_event_ptr);
        let error_listener_value = js_nanbox_pointer(error_listener);
        let _ = perry_runtime::node_stream::js_node_stream_method_remove_listener(
            handle,
            error_event,
            error_listener_value,
        );
    }
    js_promise_resolve(promise, rest_array_or_empty(rest));
    undefined_value()
}

extern "C" fn events_once_stream_reject_listener(closure: *const ClosureHeader, rest: f64) -> f64 {
    use perry_runtime::closure::js_closure_get_capture_ptr;

    let promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let handle = js_closure_get_capture_ptr(closure, 1) as Handle;
    let event_name_ptr = js_closure_get_capture_ptr(closure, 2);
    let resolve_listener = js_closure_get_capture_ptr(closure, 3);
    if handle != 0 && event_name_ptr != 0 && resolve_listener != 0 {
        let event = js_nanbox_string(event_name_ptr);
        let resolve_listener_value = js_nanbox_pointer(resolve_listener);
        let _ = perry_runtime::node_stream::js_node_stream_method_remove_listener(
            handle,
            event,
            resolve_listener_value,
        );
    }
    if !promise.is_null() {
        js_promise_reject(promise, first_rest_arg_or_undefined(rest));
    }
    undefined_value()
}

fn rest_array_or_empty(rest: f64) -> f64 {
    if JSValue::from_bits(rest.to_bits()).is_pointer() {
        rest
    } else {
        js_nanbox_pointer(js_array_alloc(0) as i64)
    }
}

fn first_rest_arg_or_undefined(rest: f64) -> f64 {
    if !JSValue::from_bits(rest.to_bits()).is_pointer() {
        return undefined_value();
    }
    let arr = js_nanbox_get_pointer(rest) as *const ArrayHeader;
    if arr.is_null() || js_array_length(arr) == 0 {
        undefined_value()
    } else {
        perry_runtime::array::js_array_get_f64(arr, 0)
    }
}

/// `events.once(emitter, eventName[, options])` — returns a Promise that resolves
/// to an array of the args fired by the next `emit(eventName, ...)`.
///
/// Node returns the *full* args array (e.g. `emit('x', 1, 2)` resolves
/// to `[1, 2]`). Perry's emit FFI today is single-arg, so the resolved
/// array is single-element. That's enough for the parity probe in
/// issue #850; multi-arg parity is a follow-up.
#[no_mangle]
pub unsafe extern "C" fn js_events_once(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    options: f64,
) -> *mut Promise {
    use perry_runtime::closure::{js_closure_alloc, js_closure_set_capture_ptr};

    ensure_gc_scanner_registered();
    let promise = js_promise_new();
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => return promise,
    };
    let signal = options_signal(options);
    if signal.is_some_and(signal_is_aborted) {
        js_promise_reject(promise, perry_runtime::url::js_abort_error_value());
        return promise;
    }
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        let mut pending = PendingOnce {
            promise,
            signal: undefined_value(),
            abort_listener: 0,
        };
        if let Some(signal) = signal {
            if let Some(signal_ptr) = object_ptr_from_value(signal) {
                let abort_listener = js_closure_alloc(events_once_abort_listener as *const u8, 2);
                js_closure_set_capture_ptr(abort_listener, 0, handle);
                js_closure_set_capture_ptr(abort_listener, 1, promise as i64);
                perry_runtime::url::js_abort_signal_add_listener(
                    signal_ptr,
                    abort_event_value(),
                    js_nanbox_pointer(abort_listener as i64),
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
        return promise;
    }
    if stream_value_from_handle(handle).is_some() {
        perry_runtime::closure::js_register_closure_rest(
            events_once_stream_resolve_listener as *const u8,
            0,
        );
        perry_runtime::closure::js_register_closure_rest(
            events_once_stream_reject_listener as *const u8,
            0,
        );
        let listener = js_closure_alloc(events_once_stream_resolve_listener as *const u8, 4);
        js_closure_set_capture_ptr(listener, 0, promise as i64);
        js_closure_set_capture_ptr(listener, 1, handle);
        js_closure_set_capture_ptr(listener, 2, 0);
        js_closure_set_capture_ptr(listener, 3, 0);
        let event_value = js_nanbox_string(event_name_ptr as i64);
        let listener_value = js_nanbox_pointer(listener as i64);
        if event_name != "error" {
            let error_event_name = b"error";
            let error_event_ptr =
                js_string_from_bytes(error_event_name.as_ptr(), error_event_name.len() as u32);
            let reject_listener =
                js_closure_alloc(events_once_stream_reject_listener as *const u8, 4);
            js_closure_set_capture_ptr(reject_listener, 0, promise as i64);
            js_closure_set_capture_ptr(reject_listener, 1, handle);
            js_closure_set_capture_ptr(reject_listener, 2, event_name_ptr as i64);
            js_closure_set_capture_ptr(reject_listener, 3, listener as i64);
            js_closure_set_capture_ptr(listener, 2, reject_listener as i64);
            js_closure_set_capture_ptr(listener, 3, error_event_ptr as i64);
            let error_event = js_nanbox_string(error_event_ptr as i64);
            let reject_listener_value = js_nanbox_pointer(reject_listener as i64);
            let _ = perry_runtime::node_stream::js_node_stream_method_once(
                handle,
                error_event,
                reject_listener_value,
            );
        }
        let _ = perry_runtime::node_stream::js_node_stream_method_once(
            handle,
            event_value,
            listener_value,
        );
    }
    promise
}

extern "C" fn events_on_queue_listener(closure: *const ClosureHeader, arg0: f64) -> f64 {
    use perry_runtime::closure::js_closure_get_capture_ptr;

    let queue = js_closure_get_capture_ptr(closure, 0) as *mut ArrayHeader;
    let abort_promise = js_closure_get_capture_ptr(closure, 1) as *mut Promise;
    if !queue.is_null() {
        let mut args = js_array_alloc(0);
        args = js_array_push_f64(args, arg0);
        let args_val = js_nanbox_pointer(args as i64);
        if abort_promise.is_null() {
            let _ = js_array_push_f64(queue, args_val);
        } else {
            let abort_val = js_nanbox_pointer(abort_promise as i64);
            let len = js_array_length(queue);
            if len == 0 {
                let _ = js_array_push_f64(queue, args_val);
                let _ = js_array_push_f64(queue, abort_val);
            } else {
                perry_runtime::array::js_array_set_f64_unchecked(queue, len - 1, args_val);
                let _ = js_array_push_f64(queue, abort_val);
            }
        }
    }

    f64::from_bits(TAG_UNDEFINED_F64_BITS)
}

extern "C" fn events_on_abort_listener(closure: *const ClosureHeader) -> f64 {
    use perry_runtime::closure::js_closure_get_capture_ptr;

    let handle = js_closure_get_capture_ptr(closure, 0) as Handle;
    let data_listener = js_closure_get_capture_ptr(closure, 1);
    let signal_ptr = js_closure_get_capture_ptr(closure, 2) as *mut ObjectHeader;
    let abort_promise = js_closure_get_capture_ptr(closure, 3) as *mut Promise;

    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        remove_listener_by_callback(emitter, data_listener);
    }
    unsafe {
        if !signal_ptr.is_null() {
            perry_runtime::url::js_abort_signal_remove_listener(
                signal_ptr,
                abort_event_value(),
                js_nanbox_pointer(closure as i64),
            );
        }
        if !abort_promise.is_null() {
            js_promise_reject(abort_promise, perry_runtime::url::js_abort_error_value());
        }
    }

    undefined_value()
}

extern "C" fn events_abort_listener_dispose(closure: *const ClosureHeader) -> f64 {
    use perry_runtime::closure::js_closure_get_capture_ptr;

    let signal_ptr = js_closure_get_capture_ptr(closure, 0);
    let callback_ptr = js_closure_get_capture_ptr(closure, 1);
    if signal_ptr != 0 && callback_ptr != 0 {
        let event_name = b"abort";
        let event_str = js_string_from_bytes(event_name.as_ptr(), event_name.len() as u32);
        let event_val = js_nanbox_string(event_str as i64);
        let listener_val = js_nanbox_pointer(callback_ptr);
        perry_runtime::url::js_abort_signal_remove_listener(
            signal_ptr as *mut perry_runtime::ObjectHeader,
            event_val,
            listener_val,
        );
    }

    f64::from_bits(TAG_UNDEFINED_F64_BITS)
}

/// `events.on(emitter, eventName[, options])` — returns an async-iterable queue of
/// argument arrays. Perry's `for await` lowering already accepts plain arrays
/// as async-iterable inputs, so the current implementation backs the iterator
/// with an Array and appends one `[arg]` entry per emitted event.
#[no_mangle]
pub unsafe extern "C" fn js_events_on(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    options: f64,
) -> *mut ArrayHeader {
    use perry_runtime::closure::{js_closure_alloc, js_closure_set_capture_ptr};

    ensure_gc_scanner_registered();
    let queue = js_array_alloc(0);
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => return queue,
    };
    let signal = options_signal(options);
    if signal.is_some_and(signal_is_aborted) {
        perry_runtime::exception::js_throw(perry_runtime::url::js_abort_error_value());
    }
    let abort_promise = if signal.is_some() {
        js_promise_new()
    } else {
        std::ptr::null_mut()
    };

    let listener = js_closure_alloc(events_on_queue_listener as *const u8, 2);
    js_closure_set_capture_ptr(listener, 0, queue as i64);
    js_closure_set_capture_ptr(listener, 1, abort_promise as i64);
    if !abort_promise.is_null() {
        let _ = js_array_push_f64(queue, js_nanbox_pointer(abort_promise as i64));
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
                perry_runtime::url::js_abort_signal_add_listener(
                    signal_ptr,
                    abort_event_value(),
                    js_nanbox_pointer(abort_listener as i64),
                );
            }
        }
    }

    queue
}

/// `events.addAbortListener(signal, listener)` — attach listener to AbortSignal
/// and return a disposable-shaped object whose `Symbol.dispose` unregisters it.
#[no_mangle]
pub unsafe extern "C" fn js_events_add_abort_listener(signal_ptr: i64, callback_ptr: i64) -> i64 {
    if signal_ptr != 0 && callback_ptr != 0 {
        use perry_runtime::closure::{js_closure_alloc, js_closure_set_capture_ptr};

        let event_name = b"abort";
        let event_str = js_string_from_bytes(event_name.as_ptr(), event_name.len() as u32);
        let event_val = js_nanbox_string(event_str as i64);
        let listener_val = js_nanbox_pointer(callback_ptr);
        perry_runtime::url::js_abort_signal_add_listener(
            signal_ptr as *mut perry_runtime::ObjectHeader,
            event_val,
            listener_val,
        );

        let dispose_closure = js_closure_alloc(events_abort_listener_dispose as *const u8, 2);
        js_closure_set_capture_ptr(dispose_closure, 0, signal_ptr);
        js_closure_set_capture_ptr(dispose_closure, 1, callback_ptr);
        let dispose_val = js_nanbox_pointer(dispose_closure as i64);

        let disposable = js_object_alloc(0, 0);
        let disposable_val = js_nanbox_pointer(disposable as i64);
        let dispose_sym = perry_runtime::symbol::well_known_symbol("dispose");
        let dispose_sym_val = js_nanbox_pointer(dispose_sym as i64);
        perry_runtime::symbol::js_object_set_symbol_property(
            disposable_val,
            dispose_sym_val,
            dispose_val,
        );
        disposable as i64
    } else {
        0
    }
}

/// `events.getEventListeners(emitter, eventName)` — alias for
/// `emitter.listeners(eventName)`.
#[no_mangle]
pub unsafe extern "C" fn js_events_get_event_listeners(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> *mut ArrayHeader {
    if let Some(target) = event_target_ptr(handle) {
        return perry_runtime::event_target::js_event_target_get_event_listeners(
            target,
            event_name_ptr,
        );
    }
    if let Some(listeners) = stream_listeners_for_heap_object(handle, event_name_ptr) {
        return listeners;
    }
    js_event_emitter_listeners(handle, event_name_ptr)
}

/// `events.listenerCount(emitter, eventName)` — alias for
/// `emitter.listenerCount(eventName)`.
#[no_mangle]
pub unsafe extern "C" fn js_events_listener_count(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> f64 {
    js_event_emitter_listener_count(handle, event_name_ptr, 0)
}

/// `events.getMaxListeners(emitter)` — alias.
#[no_mangle]
pub unsafe extern "C" fn js_events_get_max_listeners(handle: Handle) -> f64 {
    if let Some(target) = event_target_ptr(handle) {
        return perry_runtime::event_target::js_event_target_get_max_listeners(target);
    }
    js_event_emitter_get_max_listeners(handle)
}

/// `events.setMaxListeners(n, ...targets)` — codegen passes the varargs
/// target list as a Perry array of EventEmitter handles and EventTarget
/// object pointers.
#[no_mangle]
pub unsafe extern "C" fn js_events_set_max_listeners(
    n: f64,
    handles_ptr: *const ArrayHeader,
) -> f64 {
    validate_max_listeners(n);
    if !handles_ptr.is_null() {
        let len = js_array_length(handles_ptr);
        for i in 0..len {
            let handle_val = perry_runtime::array::js_array_get_f64(handles_ptr, i);
            let handle = handle_val.to_bits() as Handle;
            let handle = if (handle_val.to_bits() & 0xFFFF_0000_0000_0000) == 0x7FFD_0000_0000_0000
            {
                (handle_val.to_bits() & 0x0000_FFFF_FFFF_FFFF) as Handle
            } else {
                handle
            };
            if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
                emitter.max_listeners = n;
            } else if let Some(target) = event_target_ptr(handle) {
                let _ = perry_runtime::event_target::js_event_target_set_max_listeners(target, n);
            }
        }
    }
    f64::from_bits(TAG_UNDEFINED_F64_BITS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_scanner_emits_listeners_and_pending_promises() {
        let mut emitter = EventEmitterHandle::new();
        emitter.add_listener("data", 0x1234_5678, false, false);
        emitter
            .pending_once_promises
            .entry("ready".to_string())
            .or_default()
            .push(PendingOnce {
                promise: 0x2345_6780 as *mut Promise,
                signal: undefined_value(),
                abort_listener: 0,
            });
        let handle = register_handle(emitter);

        let mut emitted = Vec::new();
        scan_events_roots(&mut |value| emitted.push(value.to_bits()));

        assert!(emitted.contains(&(0x7FFD_0000_0000_0000 | 0x1234_5678)));
        assert!(emitted.contains(&(0x7FFD_0000_0000_0000 | 0x2345_6780)));
        crate::common::drop_handle(handle);
    }
}
