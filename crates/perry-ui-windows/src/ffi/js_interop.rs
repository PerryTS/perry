// FFI: JS interop stubs — AOT replacements for functions removed from
// perry-runtime upstream (moved to perry-jsruntime for V8 builds). These
// provide the correct AOT behavior for V8-free native builds.

/// Create a callback from a function pointer. NaN-boxes the pointer so it can
/// be stored as an f64 value and later called via js_native_call_value.
#[no_mangle]
pub extern "C" fn js_create_callback(func_ptr: i64, _closure_env: i64, _param_count: i64) -> f64 {
    perry_runtime::js_nanbox_pointer(func_ptr)
}

/// Call a JS function by module/name — no-op in AOT mode.
#[no_mangle]
pub extern "C" fn js_call_function(_module: i64, _name: i64, _args: i64, _argc: i64) -> f64 {
    f64::from_bits(perry_runtime::JSValue::undefined().bits())
}

/// Await a JS promise — in AOT mode, just pass through the value.
#[no_mangle]
pub extern "C" fn js_await_js_promise(value: f64) -> f64 {
    value
}

/// Load a JS module — no-op in AOT mode.
#[no_mangle]
pub extern "C" fn js_load_module(_path: i64) -> i64 {
    0
}

/// Construct a new instance by calling a constructor function with arguments.
#[no_mangle]
pub unsafe extern "C" fn js_new_from_handle(constructor: f64, args_ptr: i64, args_len: i64) -> f64 {
    perry_runtime::closure::js_native_call_value(
        constructor,
        args_ptr as *const f64,
        args_len as usize,
    )
}

/// Create a new instance of a class by name — no-op in pure AOT mode.
#[no_mangle]
pub extern "C" fn js_new_instance(_module: i64, _class: i64, _args: i64, _argc: i64) -> f64 {
    f64::from_bits(perry_runtime::JSValue::undefined().bits())
}

#[no_mangle]
pub extern "C" fn js_runtime_init() {}

#[no_mangle]
pub extern "C" fn js_set_property(_obj: f64, _name: i64, _value: f64) {}

#[no_mangle]
pub extern "C" fn js_get_export(_module: i64, _name: i64) -> f64 {
    f64::from_bits(perry_runtime::JSValue::undefined().bits())
}
