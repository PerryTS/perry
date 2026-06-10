use perry_runtime::{
    js_nanbox_get_pointer, js_nanbox_pointer, js_object_get_field_by_name, js_string_from_bytes,
    JSValue, ObjectHeader,
};

use crate::common::{get_handle, register_handle, Handle};

use super::{
    ensure_gc_scanner_registered, get_object_property, received, throw_invalid_arg_type,
    undefined_value, EventEmitterHandle,
};

unsafe fn event_emitter_options_capture_rejections(options: f64) -> bool {
    if !JSValue::from_bits(options.to_bits()).is_pointer() {
        return false;
    }
    let options_obj = js_nanbox_get_pointer(options) as *const ObjectHeader;
    if perry_runtime::value::addr_class::is_handle_band(options_obj as usize) {
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

/// Create a new EventEmitter.
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

unsafe fn event_emitter_async_resource_name(options: f64) -> f64 {
    let jsval = JSValue::from_bits(options.to_bits());
    if jsval.is_any_string() {
        return options;
    }

    let name = get_object_property(options, b"name").unwrap_or_else(undefined_value);
    if JSValue::from_bits(name.to_bits()).is_any_string() {
        return name;
    }

    let message = format!(
        "The \"options.name\" property must be of type string. Received {}",
        received(name)
    );
    throw_invalid_arg_type(&message)
}

#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_async_resource_new(options: f64) -> Handle {
    ensure_gc_scanner_registered();
    let name = event_emitter_async_resource_name(options);
    let async_options = if JSValue::from_bits(options.to_bits()).is_any_string() {
        undefined_value()
    } else {
        options
    };
    let async_resource_handle =
        perry_runtime::async_hooks::js_async_resource_new(name, async_options);
    let mut emitter = EventEmitterHandle::new();
    emitter.capture_rejections = event_emitter_options_capture_rejections(options);
    emitter.async_resource_handle = async_resource_handle;
    register_handle(emitter)
}

#[no_mangle]
pub extern "C" fn js_event_emitter_async_resource_call(_options: f64) -> f64 {
    let message = b"Class constructor EventEmitterAsyncResource cannot be invoked without 'new'";
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = perry_runtime::error::js_typeerror_new(msg);
    perry_runtime::exception::js_throw(perry_runtime::value::js_nanbox_pointer(err as i64))
}

#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_async_resource_async_id(handle: Handle) -> f64 {
    get_handle::<EventEmitterHandle>(handle)
        .map(|emitter| {
            perry_runtime::async_hooks::js_async_resource_async_id(emitter.async_resource_handle)
        })
        .unwrap_or(0.0)
}

#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_async_resource_trigger_async_id(handle: Handle) -> f64 {
    get_handle::<EventEmitterHandle>(handle)
        .map(|emitter| {
            perry_runtime::async_hooks::js_async_resource_trigger_async_id(
                emitter.async_resource_handle,
            )
        })
        .unwrap_or(0.0)
}

#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_async_resource_async_resource(handle: Handle) -> f64 {
    get_handle::<EventEmitterHandle>(handle)
        .and_then(|emitter| {
            if emitter.async_resource_handle == 0 {
                None
            } else {
                Some(js_nanbox_pointer(emitter.async_resource_handle))
            }
        })
        .unwrap_or_else(undefined_value)
}

#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_async_resource_emit_destroy(handle: Handle) -> f64 {
    if let Some(emitter) = get_handle::<EventEmitterHandle>(handle) {
        perry_runtime::async_hooks::js_async_resource_emit_destroy(emitter.async_resource_handle);
    }
    undefined_value()
}

pub fn is_event_emitter_async_resource_handle(handle: Handle) -> bool {
    get_handle::<EventEmitterHandle>(handle)
        .map(|emitter| emitter.async_resource_handle != 0)
        .unwrap_or(false)
}

pub(crate) fn event_emitter_async_resource_handle(handle: Handle) -> i64 {
    get_handle::<EventEmitterHandle>(handle)
        .map(|emitter| emitter.async_resource_handle)
        .unwrap_or(0)
}

// `#[used]` keepalive anchors for the EventEmitter constructor entry points.
// `new EventEmitter()` codegen calls `js_event_emitter_new_with_options`
// (builtin.rs) which is reachable only from generated `.o`; the default
// `perry file.ts -o out` auto-optimize whole-program-LLVM rebuild internalizes
// + dead-strips unreferenced `#[no_mangle]` symbols, so without an anchor the
// link fails with `Undefined symbols: _js_event_emitter_new_with_options`
// (see project_auto_optimize_keepalive_3320). Anchoring constructor shapes
// keeps `new EventEmitter()` and `new EventEmitterAsyncResource()` compiling.
#[used]
static KEEP_JS_EVENT_EMITTER_NEW: extern "C" fn() -> Handle = js_event_emitter_new;
#[used]
static KEEP_JS_EVENT_EMITTER_NEW_WITH_OPTIONS: unsafe extern "C" fn(f64) -> Handle =
    js_event_emitter_new_with_options;
#[used]
static KEEP_JS_EVENT_EMITTER_ASYNC_RESOURCE_NEW: unsafe extern "C" fn(f64) -> Handle =
    js_event_emitter_async_resource_new;
