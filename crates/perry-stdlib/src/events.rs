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
    js_closure_call2, js_nanbox_get_pointer, js_nanbox_pointer, js_nanbox_string,
    js_object_get_field_by_name_f64, js_promise_reject, js_promise_resolve, js_string_from_bytes,
    ArrayHeader, ClosureHeader, JSValue, ObjectHeader, Promise, StringHeader,
};
use std::collections::{HashMap, HashSet};

mod constructors;
use constructors::event_emitter_async_resource_handle;
pub use constructors::{
    is_event_emitter_async_resource_handle, js_event_emitter_async_resource_async_id,
    js_event_emitter_async_resource_async_resource, js_event_emitter_async_resource_call,
    js_event_emitter_async_resource_emit_destroy, js_event_emitter_async_resource_new,
    js_event_emitter_async_resource_trigger_async_id, js_event_emitter_new,
    js_event_emitter_new_with_options,
};

mod warnings;
use warnings::maybe_emit_max_listeners_warning;

mod domain;
pub use domain::{
    is_event_emitter_handle, js_event_emitter_domain_value, js_event_emitter_get_domain,
    js_event_emitter_set_domain,
};

mod handle_probes;
use handle_probes::stream_value_from_handle;

mod once_helpers;
pub use once_helpers::js_events_once;

mod events_on;
pub use events_on::js_events_on;

mod module_helpers;
pub use module_helpers::{
    js_events_add_abort_listener, js_events_get_event_listeners, js_events_get_max_listeners,
    js_events_init, js_events_listener_count, js_events_set_max_listeners,
};

const TAG_FALSE_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0003);
const TAG_TRUE_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0004);
const TAG_NULL_F64_BITS: u64 = 0x7FFC_0000_0000_0002;
const POINTER_TAG_BITS: u64 = 0x7FFD_0000_0000_0000;
const POINTER_MASK_BITS: u64 = 0x0000_FFFF_FFFF_FFFF;
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

#[derive(Clone, Copy)]
enum EventHelperTarget {
    EventEmitter(Handle),
    EventTarget(*mut ObjectHeader),
    Stream(Handle),
}

fn handle_from_value(value: f64) -> Handle {
    let bits = value.to_bits();
    if (bits & !POINTER_MASK_BITS) == POINTER_TAG_BITS {
        (bits & POINTER_MASK_BITS) as Handle
    } else {
        bits as Handle
    }
}

unsafe fn event_helper_target(value: f64) -> Option<EventHelperTarget> {
    let handle = handle_from_value(value);
    if get_handle::<EventEmitterHandle>(handle).is_some() {
        return Some(EventHelperTarget::EventEmitter(handle));
    }
    if let Some(target) = event_target_ptr(handle) {
        return Some(EventHelperTarget::EventTarget(target));
    }
    if stream_value_from_handle(handle).is_some() {
        return Some(EventHelperTarget::Stream(handle));
    }
    None
}

fn invalid_arg_type_error(message: &str) -> f64 {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    perry_runtime::node_submodules::register_error_code_pub(msg, "ERR_INVALID_ARG_TYPE");
    let err = perry_runtime::error::js_typeerror_new(msg);
    perry_runtime::value::js_nanbox_pointer(err as i64)
}

fn throw_invalid_arg_type(message: &str) -> ! {
    perry_runtime::exception::js_throw(invalid_arg_type_error(message))
}

fn received(value: f64) -> String {
    perry_runtime::fs::validate::describe_received(value)
}

fn invalid_instance_arg_message(name: &str, expected: &str, value: f64) -> String {
    format!(
        "The \"{name}\" argument must be an instance of {expected}. Received {}",
        received(value)
    )
}

fn invalid_instance_property_message(name: &str, expected: &str, value: f64) -> String {
    format!(
        "The \"{name}\" property must be an instance of {expected}. Received {}",
        received(value)
    )
}

fn invalid_type_arg_message(name: &str, expected: &str, value: f64) -> String {
    format!(
        "The \"{name}\" argument must be of type {expected}. Received {}",
        received(value)
    )
}

fn throw_invalid_emitter(value: f64) -> ! {
    throw_invalid_arg_type(&invalid_instance_arg_message(
        "emitter",
        "EventEmitter",
        value,
    ))
}

fn event_target_array_len(target: *mut ObjectHeader, event_name_ptr: *const StringHeader) -> f64 {
    let arr = unsafe {
        perry_runtime::event_target::js_event_target_get_event_listeners(target, event_name_ptr)
    };
    js_array_length(arr) as f64
}

fn format_max_listeners_received(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        }
        .to_string();
    }
    if n.fract() == 0.0 && n.abs() < 1e21 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

fn throw_max_listeners_invalid_type(value: f64) -> ! {
    let message = format!(
        "The \"setMaxListeners\" argument must be of type number. Received {}",
        perry_runtime::fs::validate::describe_received(value)
    );
    perry_runtime::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn throw_max_listeners_out_of_range(n: f64) -> ! {
    let message = format!(
        "The value of \"setMaxListeners\" is out of range. It must be >= 0. Received {}",
        format_max_listeners_received(n)
    );
    perry_runtime::fs::validate::throw_range_error_with_code(&message)
}

#[inline]
fn validate_max_listeners(value: f64) -> f64 {
    let js_value = JSValue::from_bits(value.to_bits());
    if !perry_runtime::fs::validate::is_numeric(js_value) {
        throw_max_listeners_invalid_type(value);
    }
    let n = if js_value.is_int32() {
        js_value.as_int32() as f64
    } else {
        js_value.as_number()
    };
    if n.is_nan() || n < 0.0 {
        throw_max_listeners_out_of_range(n);
    }
    n
}

use crate::common::{for_each_handle_mut_of, get_handle, get_handle_mut, Handle};

/// One registered listener: the user closure pointer (i64 to satisfy
/// Send + Sync — the underlying ClosureHeader is GC-managed), a stable raw
/// once-wrapper pointer for `rawListeners()`, plus a `once` flag.
#[derive(Copy, Clone)]
struct Listener {
    callback: i64,
    raw_wrapper: i64,
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
    /// `setMaxListeners` ceiling. Node's default is 10.
    max_listeners: f64,
    /// Event names whose current listener vector has already produced the
    /// MaxListenersExceededWarning. Cleared when that vector is drained.
    warned_events: HashSet<String>,
    /// Constructor-level `{ captureRejections: true }` flag. When enabled,
    /// rejected promises returned from listeners are routed to `"error"`.
    capture_rejections: bool,
    /// Backing AsyncResource handle for EventEmitterAsyncResource instances.
    async_resource_handle: i64,
    pub(crate) domain_handle: Option<Handle>,
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
                if l.raw_wrapper != 0 {
                    visitor.visit_i64_slot(&mut l.raw_wrapper);
                }
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
            warned_events: HashSet::new(),
            capture_rejections: false,
            async_resource_handle: 0,
            domain_handle: None,
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
            self.warned_events.remove(name);
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

    fn add_listener(
        &mut self,
        handle: Handle,
        name: &str,
        callback: i64,
        once: bool,
        prepend: bool,
    ) {
        self.emit_meta_event("newListener", name, callback);
        self.note_event(name);
        let vec = self.events.entry(name.to_string()).or_default();
        let raw_wrapper = if once {
            unsafe { create_once_raw_wrapper(handle, name, callback) }
        } else {
            0
        };
        let listener = Listener {
            callback,
            raw_wrapper,
            once,
        };
        if prepend {
            vec.insert(0, listener);
        } else {
            vec.push(listener);
        }
        maybe_emit_max_listeners_warning(self, handle, name);
    }
}

fn listener_matches(listener: &Listener, candidate: i64) -> bool {
    candidate != 0 && (listener.callback == candidate || listener.raw_wrapper == candidate)
}

fn remove_one_matching_listener(
    emitter: &mut EventEmitterHandle,
    event_name: &str,
    candidate: i64,
) -> Option<i64> {
    let removed = emitter.events.get_mut(event_name).and_then(|listeners| {
        let pos = listeners
            .iter()
            .rposition(|listener| listener_matches(listener, candidate))?;
        Some(listeners.remove(pos).callback)
    });
    if removed.is_some() {
        emitter.prune_event_if_empty(event_name);
    }
    removed
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

fn value_from_bits(bits: i64) -> f64 {
    f64::from_bits(bits as u64)
}

fn undefined_value() -> f64 {
    f64::from_bits(TAG_UNDEFINED_F64_BITS)
}

fn undefined_bits() -> i64 {
    TAG_UNDEFINED_F64_BITS as i64
}

fn event_bits_from_string_ptr(ptr: *const StringHeader) -> i64 {
    js_nanbox_string(ptr as i64).to_bits() as i64
}

unsafe fn event_name_from_bits(event_bits: i64) -> Option<String> {
    let value = value_from_bits(event_bits);
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_string() || jsval.is_short_string() {
        let ptr = perry_runtime::value::js_get_string_pointer_unified(value) as *const StringHeader;
        return string_from_header(ptr);
    }

    let ptr = perry_runtime::value::js_jsvalue_to_string(value) as *const StringHeader;
    string_from_header(ptr)
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

unsafe fn is_abort_signal_value(value: f64) -> bool {
    let Some(aborted) = get_object_property(value, b"aborted") else {
        return false;
    };
    JSValue::from_bits(aborted.to_bits()).is_bool()
}

unsafe fn validate_abort_signal_arg(value: f64, name: &str) -> f64 {
    if is_abort_signal_value(value) {
        return value;
    }
    throw_invalid_arg_type(&invalid_instance_arg_message(name, "AbortSignal", value))
}

fn closure_ptr_from_value(value: f64) -> Option<i64> {
    let bits = value.to_bits();
    if (bits & !POINTER_MASK_BITS) != POINTER_TAG_BITS {
        return None;
    }
    let ptr = (bits & POINTER_MASK_BITS) as usize;
    if ptr >= MIN_HEAP_POINTER as usize && perry_runtime::closure::is_closure_ptr(ptr) {
        Some(ptr as i64)
    } else {
        None
    }
}

fn validate_listener_arg(value: f64, name: &str) -> i64 {
    closure_ptr_from_value(value).unwrap_or_else(|| {
        throw_invalid_arg_type(&invalid_type_arg_message(name, "function", value))
    })
}

/// Validate an EventEmitter instance-method listener argument (#3072).
///
/// `listener_bits` is the raw NaN-box bit pattern delivered by the codegen
/// `NA_JSV` slot. Returns the closure pointer when callable, otherwise throws
/// `TypeError [ERR_INVALID_ARG_TYPE]`. Delegates to the shared runtime
/// validator so `on` / `once` / `addListener` / `prependListener` /
/// `prependOnceListener` / `removeListener` / `off` all produce Node's exact
/// error class, code and message.
fn validate_event_listener(listener_bits: i64) -> i64 {
    const NAME: &[u8] = b"listener";
    unsafe {
        perry_runtime::fs::validate::js_validate_event_listener(
            listener_bits,
            NAME.as_ptr(),
            NAME.len() as u32,
        )
    }
}

static RAW_ONCE_WRAPPER_REST_REGISTERED: std::sync::Once = std::sync::Once::new();

fn ensure_raw_once_wrapper_rest_registered() {
    RAW_ONCE_WRAPPER_REST_REGISTERED.call_once(|| {
        perry_runtime::closure::js_register_closure_rest(
            event_emitter_once_wrapper as *const u8,
            0,
        );
    });
}

unsafe fn create_once_raw_wrapper(handle: Handle, event_name: &str, callback: i64) -> i64 {
    if callback == 0 {
        return 0;
    }
    ensure_raw_once_wrapper_rest_registered();

    let wrapper =
        perry_runtime::closure::js_closure_alloc(event_emitter_once_wrapper as *const u8, 4);
    let event_ptr = js_string_from_bytes(event_name.as_ptr(), event_name.len() as u32);
    perry_runtime::closure::js_closure_set_capture_ptr(wrapper, 0, handle);
    perry_runtime::closure::js_closure_set_capture_ptr(wrapper, 1, event_ptr as i64);
    perry_runtime::closure::js_closure_set_capture_ptr(wrapper, 2, callback);
    perry_runtime::closure::js_closure_set_capture_ptr(wrapper, 3, wrapper as i64);

    perry_runtime::closure::closure_set_dynamic_prop(
        wrapper as usize,
        "listener",
        js_nanbox_pointer(callback),
    );
    let name = b"bound onceWrapper";
    let name_ptr = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    perry_runtime::closure::closure_set_dynamic_prop(
        wrapper as usize,
        "name",
        js_nanbox_string(name_ptr as i64),
    );

    wrapper as i64
}

extern "C" fn event_emitter_once_wrapper(closure: *const ClosureHeader, rest: f64) -> f64 {
    use perry_runtime::closure::js_closure_get_capture_ptr;

    unsafe {
        let handle = js_closure_get_capture_ptr(closure, 0) as Handle;
        let event_name_ptr = js_closure_get_capture_ptr(closure, 1) as *const StringHeader;
        let callback = js_closure_get_capture_ptr(closure, 2);
        let wrapper = js_closure_get_capture_ptr(closure, 3);
        if handle != 0 && callback != 0 {
            if let Some(event_name) = string_from_header(event_name_ptr) {
                let removed = get_handle_mut::<EventEmitterHandle>(handle).and_then(|emitter| {
                    remove_one_matching_listener(emitter, &event_name, wrapper)
                });
                if removed.is_some() {
                    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
                        emitter.emit_meta_event("removeListener", &event_name, callback);
                    }
                }
            }
        }

        let args_ptr = if JSValue::from_bits(rest.to_bits()).is_pointer() {
            js_nanbox_get_pointer(rest) as *const ArrayHeader
        } else {
            std::ptr::null()
        };
        let args = collect_emit_args(args_ptr);
        if callback == 0 {
            undefined_value()
        } else {
            let async_resource_handle = event_emitter_async_resource_handle(handle);
            call_emitter_listener(handle, async_resource_handle, callback, &args)
        }
    }
}

unsafe fn options_signal_result(options: f64) -> Result<Option<f64>, f64> {
    let jsval = JSValue::from_bits(options.to_bits());
    if jsval.is_undefined() {
        return Ok(None);
    }
    if object_ptr_from_value(options).is_none() {
        return Err(invalid_arg_type_error(&invalid_type_arg_message(
            "options", "object", options,
        )));
    }
    let Some(signal) = get_object_property(options, b"signal") else {
        return Ok(None);
    };
    if is_abort_signal_value(signal) {
        Ok(Some(signal))
    } else {
        Err(invalid_arg_type_error(&invalid_instance_property_message(
            "options.signal",
            "AbortSignal",
            signal,
        )))
    }
}

unsafe fn options_signal_or_throw(options: f64) -> Option<f64> {
    match options_signal_result(options) {
        Ok(signal) => signal,
        Err(error) => perry_runtime::exception::js_throw(error),
    }
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
            listeners.retain(|listener| !listener_matches(listener, callback));
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

/// EventEmitter.on(eventName, listener) — also serves as `addListener`.
/// Register a listener for the specified event.
/// Returns the emitter handle for chaining.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_on(
    handle: Handle,
    event_bits: i64,
    listener_bits: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    // #3072: throw TypeError [ERR_INVALID_ARG_TYPE] for a non-function
    // listener before touching the event name (Node validates listener first).
    let callback_ptr = validate_event_listener(listener_bits);
    let event_name = match event_name_from_bits(event_bits) {
        Some(name) => name,
        None => return handle,
    };
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter.add_listener(handle, &event_name, callback_ptr, false, false);
    }
    handle
}

/// EventEmitter.once(eventName, listener) — fires once, then auto-removes.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_once(
    handle: Handle,
    event_bits: i64,
    listener_bits: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    let callback_ptr = validate_event_listener(listener_bits);
    let event_name = match event_name_from_bits(event_bits) {
        Some(name) => name,
        None => return handle,
    };
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter.add_listener(handle, &event_name, callback_ptr, true, false);
    }
    handle
}

/// EventEmitter.prependListener(eventName, listener) — insert at front.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_prepend_listener(
    handle: Handle,
    event_bits: i64,
    listener_bits: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    let callback_ptr = validate_event_listener(listener_bits);
    let event_name = match event_name_from_bits(event_bits) {
        Some(name) => name,
        None => return handle,
    };
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter.add_listener(handle, &event_name, callback_ptr, false, true);
    }
    handle
}

/// EventEmitter.prependOnceListener(eventName, listener).
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_prepend_once_listener(
    handle: Handle,
    event_bits: i64,
    listener_bits: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    let callback_ptr = validate_event_listener(listener_bits);
    let event_name = match event_name_from_bits(event_bits) {
        Some(name) => name,
        None => return handle,
    };
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter.add_listener(handle, &event_name, callback_ptr, true, true);
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

unsafe fn call_emitter_listener(
    handle: Handle,
    async_resource_handle: i64,
    callback: i64,
    args: &[f64],
) -> f64 {
    let receiver = js_nanbox_pointer(handle);
    let callback_value = js_nanbox_pointer(callback);
    if async_resource_handle != 0 {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let arg_handles = scope.root_nanbox_f64_slice(args);
        let arr = js_array_alloc(0);
        let arr_handle = scope.root_raw_mut_ptr(arr);
        for arg in &arg_handles {
            let arr = js_array_push_f64(arr_handle.get_raw_mut_ptr(), arg.get_nanbox_f64());
            arr_handle.set_raw_mut_ptr(arr);
        }
        return perry_runtime::async_hooks::js_async_resource_run_in_async_scope(
            async_resource_handle,
            callback_value,
            receiver,
            arr_handle.get_raw_mut_ptr::<ArrayHeader>() as i64,
        );
    }
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
            js_event_emitter_emit(handle, event_bits_from_string_ptr(event_str), args);
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
    event_bits: i64,
    args_ptr: *mut ArrayHeader,
) -> f64 {
    let event_name = match event_name_from_bits(event_bits) {
        Some(name) => name,
        None => return TAG_FALSE_F64,
    };

    let mut had_listeners = false;
    let mut domain_error: Option<(Handle, f64)> = None;
    let mut throw_error: Option<f64> = None;
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
                if let Some(domain) = emitter.domain_handle {
                    domain_error = Some((domain, first_arg));
                } else {
                    throw_error = Some(first_arg);
                }
            }
        }

        if domain_error.is_none() && throw_error.is_none() {
            // Resolve any pending `events.once` Promises before dispatch.
            drain_pending_once_promises(emitter, &event_name, args_ptr);

            let capture_rejections = emitter.capture_rejections && event_name != "error";
            let async_handle = emitter.async_resource_handle;
            for l in snapshot {
                if l.callback != 0 {
                    let result =
                        call_emitter_listener(handle, async_handle, l.callback, &emitted_args);
                    if capture_rejections {
                        capture_listener_rejection(handle, result);
                    }
                }
            }
        }
    }

    if let Some((domain, error)) = domain_error {
        let _ =
            crate::domain::js_domain_emit_error(domain, error, js_nanbox_pointer(handle), false);
        return TAG_FALSE_F64;
    }
    if let Some(error) = throw_error {
        perry_runtime::exception::js_throw(error);
    }

    bool_to_js(had_listeners)
}

/// EventEmitter.emit with no arguments
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_emit0(handle: Handle, event_bits: i64) -> f64 {
    let event_name = match event_name_from_bits(event_bits) {
        Some(name) => name,
        None => return TAG_FALSE_F64,
    };

    let mut had_listeners = false;
    let mut domain_error: Option<(Handle, f64)> = None;
    let mut throw_error: Option<f64> = None;
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
            let error_value = f64::from_bits(TAG_UNDEFINED_F64_BITS);
            dispatch_error_monitor(emitter, None);
            let has_error_once = emitter
                .pending_once_promises
                .get("error")
                .is_some_and(|pending| !pending.is_empty());
            let rejected_once = reject_pending_once_promises_for_error(emitter, error_value);
            had_listeners = had_listeners || has_error_once || rejected_once;
            if snapshot.is_empty() && !has_error_once && !rejected_once {
                if let Some(domain) = emitter.domain_handle {
                    domain_error = Some((domain, error_value));
                } else {
                    throw_error = Some(error_value);
                }
            }
        }
        if domain_error.is_none() && throw_error.is_none() {
            drain_pending_once_promises(emitter, &event_name, empty_args);

            let capture_rejections = emitter.capture_rejections && event_name != "error";
            let async_handle = emitter.async_resource_handle;
            for l in snapshot {
                if l.callback != 0 {
                    let result = call_emitter_listener(handle, async_handle, l.callback, &[]);
                    if capture_rejections {
                        capture_listener_rejection(handle, result);
                    }
                }
            }
        }
    }

    if let Some((domain, error)) = domain_error {
        let _ =
            crate::domain::js_domain_emit_error(domain, error, js_nanbox_pointer(handle), false);
        return TAG_FALSE_F64;
    }
    if let Some(error) = throw_error {
        perry_runtime::exception::js_throw(error);
    }

    bool_to_js(had_listeners)
}

/// EventEmitter.removeListener(eventName, listener)
/// Remove the most recently added matching listener for the event.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_remove_listener(
    handle: Handle,
    event_bits: i64,
    listener_bits: i64,
) -> Handle {
    // #3072: `removeListener`/`off` also require a callable listener; Node
    // throws TypeError [ERR_INVALID_ARG_TYPE] for non-functions before any
    // table lookup.
    let callback_ptr = validate_event_listener(listener_bits);
    let event_name = match event_name_from_bits(event_bits) {
        Some(name) => name,
        None => return handle,
    };

    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        // Node removes only one matching listener: the most recently added
        // instance among duplicates. A raw once wrapper removes that exact
        // wrapper; the original callback removes the most recent wrapper or
        // regular listener carrying that original callback.
        if let Some(removed_callback) =
            remove_one_matching_listener(emitter, &event_name, callback_ptr)
        {
            emitter.emit_meta_event("removeListener", &event_name, removed_callback);
        }
    }
    handle
}

/// EventEmitter.removeAllListeners(eventName?)
/// Remove all listeners for an event (or all events if no name given).
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_remove_all_listeners(
    handle: Handle,
    args_ptr: *const ArrayHeader,
) -> Handle {
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        if args_ptr.is_null() || js_array_length(args_ptr) == 0 {
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
            emitter.warned_events.clear();
            for (name, callback) in removed {
                emitter.emit_meta_event("removeListener", &name, callback);
            }
        } else if let Some(event_name) = event_name_from_bits(
            perry_runtime::array::js_array_get_f64(args_ptr, 0).to_bits() as i64,
        ) {
            let removed: Vec<i64> = emitter
                .events
                .get(&event_name)
                .map(|listeners| listeners.iter().map(|listener| listener.callback).collect())
                .unwrap_or_default();
            emitter.events.remove(&event_name);
            emitter.warned_events.remove(&event_name);
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
    event_bits: i64,
    listener_bits: i64,
) -> f64 {
    let event_name = match event_name_from_bits(event_bits) {
        Some(name) => name,
        None => return 0.0,
    };
    let listener_value = value_from_bits(listener_bits);
    let listener_value = JSValue::from_bits(listener_value.to_bits());
    let listener_filter = if listener_value.is_undefined() || listener_value.is_null() {
        None
    } else {
        Some(closure_ptr_from_value(value_from_bits(listener_bits)).unwrap_or(0))
    };
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        if let Some(listeners) = emitter.events.get(&event_name) {
            if let Some(callback_ptr) = listener_filter {
                if callback_ptr == 0 {
                    return 0.0;
                }
                return listeners
                    .iter()
                    .filter(|listener| listener_matches(listener, callback_ptr))
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
    let n = validate_max_listeners(n);
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
    event_bits: i64,
) -> *mut ArrayHeader {
    let arr = js_array_alloc(0);
    let event_name = match event_name_from_bits(event_bits) {
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

/// EventEmitter.rawListeners(eventName) — returns stored once wrappers while
/// `listeners()` returns the original user closures.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_raw_listeners(
    handle: Handle,
    event_bits: i64,
) -> *mut ArrayHeader {
    let arr = js_array_alloc(0);
    let event_name = match event_name_from_bits(event_bits) {
        Some(name) => name,
        None => return arr,
    };
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        if let Some(listeners) = emitter.events.get(&event_name) {
            let mut result = arr;
            for l in listeners.iter() {
                let callback = if l.once && l.raw_wrapper != 0 {
                    l.raw_wrapper
                } else {
                    l.callback
                };
                if callback != 0 {
                    let nanboxed = js_nanbox_pointer(callback);
                    result = js_array_push_f64(result, nanboxed);
                }
            }
            return result;
        }
    }
    arr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_scanner_emits_listeners_and_pending_promises() {
        let mut emitter = EventEmitterHandle::new();
        emitter.add_listener(0, "data", 0x1234_5678, false, false);
        emitter
            .pending_once_promises
            .entry("ready".to_string())
            .or_default()
            .push(PendingOnce {
                promise: 0x2345_6780 as *mut Promise,
                signal: undefined_value(),
                abort_listener: 0,
            });
        let handle = crate::common::register_handle(emitter);

        let mut emitted = Vec::new();
        scan_events_roots(&mut |value| emitted.push(value.to_bits()));

        assert!(emitted.contains(&(0x7FFD_0000_0000_0000 | 0x1234_5678)));
        assert!(emitted.contains(&(0x7FFD_0000_0000_0000 | 0x2345_6780)));
        crate::common::drop_handle(handle);
    }
}
