//! `node:vm` direct-call FFI wrappers.
//!
//! The actual behavior lives in `perry_runtime::node_vm` so
//! namespace property reads and bound callables behave the same way even when
//! `node:vm` is reached through `process.getBuiltinModule("vm")`.

fn receiver_value(receiver: i64) -> f64 {
    perry_runtime::js_nanbox_pointer(receiver)
}

#[no_mangle]
pub extern "C" fn js_vm_script_call(code: f64, options: f64) -> f64 {
    perry_runtime::node_vm::js_vm_script_call(code, options)
}

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

#[no_mangle]
pub extern "C" fn js_vm_module_call() -> f64 {
    perry_runtime::node_vm::js_vm_module_call()
}

#[no_mangle]
pub extern "C" fn js_vm_source_text_module_new(code: f64, options: f64) -> f64 {
    perry_runtime::node_vm::js_vm_source_text_module_new(code, options)
}

#[no_mangle]
pub extern "C" fn js_vm_synthetic_module_new(
    export_names: f64,
    evaluate_callback: f64,
    options: f64,
) -> f64 {
    perry_runtime::node_vm::js_vm_synthetic_module_new(export_names, evaluate_callback, options)
}

#[no_mangle]
pub extern "C" fn js_vm_module_status(module: i64) -> f64 {
    perry_runtime::node_vm::js_vm_module_status(receiver_value(module))
}

#[no_mangle]
pub extern "C" fn js_vm_module_identifier(module: i64) -> f64 {
    perry_runtime::node_vm::js_vm_module_identifier(receiver_value(module))
}

#[no_mangle]
pub extern "C" fn js_vm_module_error(module: i64) -> f64 {
    perry_runtime::node_vm::js_vm_module_error(receiver_value(module))
}

#[no_mangle]
pub extern "C" fn js_vm_module_namespace(module: i64) -> f64 {
    perry_runtime::node_vm::js_vm_module_namespace(receiver_value(module))
}

#[no_mangle]
pub extern "C" fn js_vm_module_link(module: i64, linker: f64) -> f64 {
    perry_runtime::node_vm::js_vm_module_link(receiver_value(module), linker)
}

#[no_mangle]
pub extern "C" fn js_vm_module_evaluate(module: i64, options: f64) -> f64 {
    perry_runtime::node_vm::js_vm_module_evaluate(receiver_value(module), options)
}

#[no_mangle]
pub extern "C" fn js_vm_source_text_module_dependency_specifiers(module: i64) -> f64 {
    perry_runtime::node_vm::js_vm_source_text_module_dependency_specifiers(receiver_value(module))
}

#[no_mangle]
pub extern "C" fn js_vm_source_text_module_module_requests(module: i64) -> f64 {
    perry_runtime::node_vm::js_vm_source_text_module_module_requests(receiver_value(module))
}

#[no_mangle]
pub extern "C" fn js_vm_source_text_module_create_cached_data(module: i64) -> f64 {
    perry_runtime::node_vm::js_vm_source_text_module_create_cached_data(receiver_value(module))
}

#[no_mangle]
pub extern "C" fn js_vm_source_text_module_link_requests(module: i64, modules: f64) -> f64 {
    perry_runtime::node_vm::js_vm_source_text_module_link_requests(receiver_value(module), modules)
}

#[no_mangle]
pub extern "C" fn js_vm_source_text_module_instantiate(module: i64) -> f64 {
    perry_runtime::node_vm::js_vm_source_text_module_instantiate(receiver_value(module))
}

#[no_mangle]
pub extern "C" fn js_vm_source_text_module_has_top_level_await(module: i64) -> f64 {
    perry_runtime::node_vm::js_vm_source_text_module_has_top_level_await(receiver_value(module))
}

#[no_mangle]
pub extern "C" fn js_vm_source_text_module_has_async_graph(module: i64) -> f64 {
    perry_runtime::node_vm::js_vm_source_text_module_has_async_graph(receiver_value(module))
}

#[no_mangle]
pub extern "C" fn js_vm_synthetic_module_set_export(module: i64, name: f64, value: f64) -> f64 {
    perry_runtime::node_vm::js_vm_synthetic_module_set_export(receiver_value(module), name, value)
}
