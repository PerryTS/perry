//! node:events module-level helpers: `events.once`, `events.on`,
//! `events.getEventListeners`, `events.listenerCount`,
//! `events.getMaxListeners`, `events.setMaxListeners`,
//! `events.addAbortListener` and the legacy `events.init`.
//!
//! An emitter is an ordinary object whose methods live on the shared
//! `EventEmitter.prototype` (perry-runtime, #10508): `new EventEmitter()`, a
//! subclass instance, a stream, a socket and an `Object.create(
//! EventEmitter.prototype)` mixin are all reached the same way. Like node's
//! own helpers, these act on an emitter only through its methods (`once`,
//! `removeListener`, `listeners`, ...), so a subclass override is honoured and
//! no listener state lives anywhere but the emitter's own `_events`.

use perry_runtime::{
    js_array_length, js_nanbox_get_pointer, js_nanbox_pointer, js_nanbox_string,
    js_object_get_field_by_name_f64, js_string_from_bytes, ArrayHeader, JSValue, ObjectHeader,
    StringHeader,
};

mod once_helpers;
pub use once_helpers::js_events_once;

mod events_on;
pub use events_on::js_events_on;

mod module_helpers;
pub use module_helpers::{
    js_events_add_abort_listener, js_events_get_event_listeners, js_events_get_max_listeners,
    js_events_init, js_events_listener_count, js_events_native_dispatch,
    js_events_set_max_listeners,
};

const TAG_FALSE_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0003);
const TAG_TRUE_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0004);
const TAG_UNDEFINED_F64_BITS: u64 = 0x7FFC_0000_0000_0001;
const POINTER_TAG_BITS: u64 = 0x7FFD_0000_0000_0000;
const POINTER_MASK_BITS: u64 = 0x0000_FFFF_FFFF_FFFF;
const MIN_HEAP_POINTER: u64 = 0x10000;
const MAX_HEAP_POINTER: u64 = 0x0000_FFFF_FFFF_FFFF;

/// What a module-level helper was handed.
#[derive(Clone, Copy)]
enum EventHelperTarget {
    /// Anything with emitter methods: an `EventEmitter` (or subclass, mixin,
    /// stream) object, or a `net.Socket` handle. Driven by its methods.
    Emitter(f64),
    /// A web `EventTarget`, driven by its native listener list.
    EventTarget(*mut ObjectHeader),
}

/// The `EventTarget` object `value` names, if it is one.
unsafe fn event_target_ptr(value: f64) -> Option<*mut ObjectHeader> {
    let bits = value.to_bits();
    if (bits & !POINTER_MASK_BITS) != POINTER_TAG_BITS {
        return None;
    }
    let addr = bits & POINTER_MASK_BITS;
    if !(MIN_HEAP_POINTER..=MAX_HEAP_POINTER).contains(&addr) || addr & 0x7 != 0 {
        return None;
    }
    let ptr = addr as *mut ObjectHeader;
    if perry_runtime::event_target::js_event_target_is_event_target(ptr) != 0 {
        Some(ptr)
    } else {
        None
    }
}

/// node's emitter test for its helpers: an object whose `on` is callable (a
/// `net.Socket` handle answers through the handle dispatch).
unsafe fn event_helper_target(value: f64) -> Option<EventHelperTarget> {
    if let Some(target) = event_target_ptr(value) {
        return Some(EventHelperTarget::EventTarget(target));
    }
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return None;
    }
    let addr = (value.to_bits() & POINTER_MASK_BITS) as i64;
    if !perry_runtime::value::addr_class::is_above_handle_band(addr as usize) {
        let is_socket = perry_runtime::object::net_socket_handle_probe()
            .is_some_and(|probe| unsafe { probe(addr) });
        return is_socket.then_some(EventHelperTarget::Emitter(value));
    }
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let key = scope.root_nanbox_f64(name_value("on"));
    let on = perry_runtime::object::js_object_get_property_key(
        value.get_nanbox_f64(),
        key.get_nanbox_f64(),
    );
    if closure_ptr_from_value(on).is_some() {
        Some(EventHelperTarget::Emitter(value.get_nanbox_f64()))
    } else {
        None
    }
}

/// A runtime-owned property / event name as a JS string value.
fn name_value(name: &str) -> f64 {
    js_nanbox_string(js_string_from_bytes(name.as_ptr(), name.len() as u32) as i64)
}

/// The event-name value `ptr` names: a string, or a symbol (whose header the
/// codegen hands over in the same slot).
unsafe fn event_value(ptr: *const StringHeader) -> f64 {
    if ptr.is_null() {
        return undefined_value();
    }
    let sym_ptr = ptr as *const perry_runtime::symbol::SymbolHeader;
    if (*sym_ptr).magic == perry_runtime::symbol::SYMBOL_MAGIC {
        return js_nanbox_pointer(ptr as i64);
    }
    js_nanbox_string(ptr as i64)
}

/// `target[name](...args)` with `this = target`: an emitter is driven only
/// through its own (possibly overridden) methods, as node's helpers do.
unsafe fn call_emitter_method(target: f64, name: &str, args: &[f64]) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let args = scope.root_nanbox_f64_slice(args);
    let name = scope.root_string_ptr(js_string_from_bytes(name.as_ptr(), name.len() as u32));
    let args = perry_runtime::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&args);
    perry_runtime::object::js_native_call_method_str_key(
        target.get_nanbox_f64(),
        name.get_raw_const_ptr::<StringHeader>() as i64,
        args.as_ptr(),
        args.len(),
    )
}

/// A method result that is an array, else a fresh empty array.
fn result_array(value: f64) -> *mut ArrayHeader {
    let bits = value.to_bits();
    if (bits & !POINTER_MASK_BITS) == POINTER_TAG_BITS && (bits & POINTER_MASK_BITS) != 0 {
        (bits & POINTER_MASK_BITS) as *mut ArrayHeader
    } else {
        perry_runtime::js_array_alloc(0)
    }
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

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() || perry_runtime::value::addr_class::is_handle_band(ptr as usize) {
        return None;
    }

    let sym_ptr = ptr as *const perry_runtime::symbol::SymbolHeader;
    if (*sym_ptr).magic == perry_runtime::symbol::SYMBOL_MAGIC {
        let sym_value = js_nanbox_pointer(ptr as i64);
        let rendered = perry_runtime::symbol::js_symbol_to_string(sym_value);
        return string_from_header(rendered as *const StringHeader);
    }

    crate::common::string_from_header_lossy(ptr)
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
    name_value("abort")
}

/// `signal.removeEventListener('abort', listener)`.
unsafe fn remove_abort_listener(signal: f64, listener: f64) {
    let Some(signal_ptr) = object_ptr_from_value(signal) else {
        return;
    };
    perry_runtime::url::js_abort_signal_remove_listener(signal_ptr, abort_event_value(), listener);
}
