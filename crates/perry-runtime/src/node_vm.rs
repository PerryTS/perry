//! `node:vm` import/require scaffold.
//!
//! This module intentionally exposes only the module-shape layer Perry can
//! model without a runtime JavaScript interpreter: function-valued exports,
//! the `Script` constructor value, `isContext({}) === false`, and the
//! `vm.constants` symbol namespace. Script execution, contextification,
//! compileFunction, measureMemory, cached data, source maps, and VM modules
//! remain tracked by the VM issues referenced from the parity fixtures.

use crate::value::JSValue;

fn bool_value(value: bool) -> f64 {
    f64::from_bits(JSValue::bool(value).bits())
}

fn throw_vm_unimplemented(api: &str, issue: &str) -> f64 {
    let message = format!("node:vm {api} is not implemented in Perry (tracked by #{issue}).");
    crate::fs::validate::throw_error_with_code(&message, "ERR_PERRY_VM_UNIMPLEMENTED")
}

// `createContext` is handled by the working implementation in
// `object::native_module.rs` (#4050); routed there from dispatch.

pub extern "C" fn js_vm_create_script(_code: f64, _options: f64) -> f64 {
    throw_vm_unimplemented("createScript/Script compilation", "3127")
}

pub extern "C" fn js_vm_run_in_context(
    _code: f64,
    _contextified_object: f64,
    _options: f64,
) -> f64 {
    throw_vm_unimplemented("runInContext execution", "3128")
}

pub extern "C" fn js_vm_run_in_new_context(_code: f64, _context_object: f64, _options: f64) -> f64 {
    throw_vm_unimplemented("runInNewContext execution", "3128")
}

pub extern "C" fn js_vm_run_in_this_context(_code: f64, _options: f64) -> f64 {
    throw_vm_unimplemented("runInThisContext execution", "3127")
}

pub extern "C" fn js_vm_is_context(_object: f64) -> f64 {
    bool_value(false)
}

pub extern "C" fn js_vm_compile_function(_code: f64, _params: f64, _options: f64) -> f64 {
    throw_vm_unimplemented("compileFunction runtime function construction", "3130")
}

pub extern "C" fn js_vm_measure_memory(_options: f64) -> f64 {
    throw_vm_unimplemented("measureMemory", "3284")
}

pub extern "C" fn js_vm_script_call(_code: f64, _options: f64) -> f64 {
    throw_vm_unimplemented("Script constructor execution", "3127")
}

/// Dispatch a `node:vm` module method reached as a value/namespace call
/// (e.g. `vm.createScript(...)` or a bound export). `createContext` routes to
/// the working #4050 contextification helper; the rest are the shape-only
/// scaffold (#4079) plus measureMemory validation (#4087).
pub fn dispatch_vm_method(method: &str, arg0: f64, arg1: f64, arg2: f64) -> f64 {
    match method {
        "Script" => js_vm_script_call(arg0, arg1),
        "createContext" => crate::object::js_vm_create_context(arg0),
        "createScript" => js_vm_create_script(arg0, arg1),
        "runInContext" => js_vm_run_in_context(arg0, arg1, arg2),
        "runInNewContext" => js_vm_run_in_new_context(arg0, arg1, arg2),
        "runInThisContext" => js_vm_run_in_this_context(arg0, arg1),
        "isContext" => js_vm_is_context(arg0),
        "compileFunction" => js_vm_compile_function(arg0, arg1, arg2),
        "measureMemory" => js_vm_measure_memory(arg0),
        _ => f64::from_bits(JSValue::undefined().bits()),
    }
}
