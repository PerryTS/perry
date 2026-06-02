//! `node:vm` direct-call FFI wrappers.
//!
//! The actual scaffold behavior lives in `perry_runtime::node_vm` so
//! namespace property reads and bound callables behave the same way even when
//! `node:vm` is reached through `process.getBuiltinModule("vm")`.

// `js_vm_create_context` is provided by perry-runtime (#4050) as a working
// 1-arg contextification helper; do not redefine it here or the `#[no_mangle]`
// symbol collides at link time.

#[no_mangle]
pub extern "C" fn js_vm_create_script(code: f64, options: f64) -> f64 {
    perry_runtime::node_vm::js_vm_create_script(code, options)
}

#[no_mangle]
pub extern "C" fn js_vm_run_in_context(code: f64, contextified_object: f64, options: f64) -> f64 {
    perry_runtime::node_vm::js_vm_run_in_context(code, contextified_object, options)
}

#[no_mangle]
pub extern "C" fn js_vm_run_in_new_context(code: f64, context_object: f64, options: f64) -> f64 {
    perry_runtime::node_vm::js_vm_run_in_new_context(code, context_object, options)
}

#[no_mangle]
pub extern "C" fn js_vm_run_in_this_context(code: f64, options: f64) -> f64 {
    perry_runtime::node_vm::js_vm_run_in_this_context(code, options)
}

#[no_mangle]
pub extern "C" fn js_vm_is_context(object: f64) -> f64 {
    perry_runtime::node_vm::js_vm_is_context(object)
}

#[no_mangle]
pub extern "C" fn js_vm_compile_function(code: f64, params: f64, options: f64) -> f64 {
    perry_runtime::node_vm::js_vm_compile_function(code, params, options)
}

#[no_mangle]
pub extern "C" fn js_vm_measure_memory(options: f64) -> f64 {
    perry_runtime::node_vm::js_vm_measure_memory(options)
}
