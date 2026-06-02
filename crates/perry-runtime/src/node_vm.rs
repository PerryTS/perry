//! `node:vm` import/require scaffold.
//!
//! This module intentionally exposes only the module-shape layer Perry can
//! model without a runtime JavaScript interpreter: function-valued exports,
//! the `Script` constructor value, `isContext({}) === false`, and the
//! `vm.constants` symbol namespace. Script execution, contextification,
//! compileFunction, cached data, source maps, and VM modules remain tracked by
//! the VM issues referenced from the parity fixtures.

use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::object::ObjectHeader;
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;
use std::sync::atomic::{AtomicBool, Ordering};

static MEASURE_MEMORY_WARNING_EMITTED: AtomicBool = AtomicBool::new(false);

fn bool_value(value: bool) -> f64 {
    f64::from_bits(JSValue::bool(value).bits())
}

fn undefined_value() -> f64 {
    f64::from_bits(JSValue::undefined().bits())
}

fn string_value(value: &str) -> f64 {
    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn boxed_ptr(ptr: *const u8) -> f64 {
    f64::from_bits(JSValue::pointer(ptr).bits())
}

fn throw_vm_unimplemented(api: &str, issue: &str) -> f64 {
    let message = format!("node:vm {api} is not implemented in Perry (tracked by #{issue}).");
    crate::fs::validate::throw_error_with_code(&message, "ERR_PERRY_VM_UNIMPLEMENTED")
}

// `createContext` is handled by the working implementation in
// `object::native_module.rs` (#4050); routed there from dispatch.

fn object_ptr_from_value(value: f64) -> Option<*const ObjectHeader> {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let gc_header = unsafe { &*(ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader) };
    if gc_header.obj_type != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    Some(ptr as *const ObjectHeader)
}

fn validate_options_object(options: f64) {
    let jv = JSValue::from_bits(options.to_bits());
    if jv.is_undefined() || object_ptr_from_value(options).is_some() {
        return;
    }
    let message = format!(
        "The \"options\" argument must be of type object. Received {}",
        crate::fs::validate::describe_received(options)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

fn object_property<'scope>(
    scope: &'scope RuntimeHandleScope,
    options_handle: &RuntimeHandle<'scope>,
    name: &[u8],
) -> f64 {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let key_handle = scope.root_string_ptr(key);
    let Some(obj) = object_ptr_from_value(options_handle.get_nanbox_f64()) else {
        return undefined_value();
    };
    crate::object::js_object_get_field_by_name_f64(
        obj,
        key_handle.get_raw_const_ptr::<StringHeader>(),
    )
}

fn string_from_value(value: f64) -> Option<String> {
    let js_value = JSValue::from_bits(value.to_bits());
    if !js_value.is_any_string() {
        return None;
    }
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return Some(String::new());
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
}

fn string_value_is(value: f64, expected: &str) -> bool {
    string_from_value(value).is_some_and(|actual| actual == expected)
}

fn format_number_for_received(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value.is_sign_positive() {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        };
    }
    if value.fract() == 0.0 && value.abs() < 1e21 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

fn format_validate_one_of_received(value: f64) -> String {
    let jv = JSValue::from_bits(value.to_bits());
    if let Some(value) = string_from_value(value) {
        return format!("'{value}'");
    }
    if jv.is_null() {
        return "null".to_string();
    }
    if jv.is_undefined() {
        return "undefined".to_string();
    }
    if jv.is_bool() {
        return format!("{}", jv.as_bool());
    }
    if jv.is_int32() {
        return format!("{}", jv.as_int32());
    }
    if jv.is_number() {
        return format_number_for_received(jv.as_number());
    }
    crate::fs::validate::describe_received(value)
}

fn throw_invalid_one_of(property: &str, allowed: &[&str], value: f64) -> ! {
    let allowed = allowed
        .iter()
        .map(|entry| format!("'{entry}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let message = format!(
        "The property '{property}' must be one of: {allowed}. Received {}",
        format_validate_one_of_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MeasureMemoryMode {
    Summary,
    Detailed,
}

fn validate_measure_memory_options(options: f64) -> MeasureMemoryMode {
    validate_options_object(options);
    if JSValue::from_bits(options.to_bits()).is_undefined() {
        return MeasureMemoryMode::Summary;
    }

    let scope = RuntimeHandleScope::new();
    let options_handle = scope.root_nanbox_f64(options);
    let mode = object_property(&scope, &options_handle, b"mode");
    let execution = object_property(&scope, &options_handle, b"execution");

    let mode =
        if JSValue::from_bits(mode.to_bits()).is_undefined() || string_value_is(mode, "summary") {
            MeasureMemoryMode::Summary
        } else if string_value_is(mode, "detailed") {
            MeasureMemoryMode::Detailed
        } else {
            throw_invalid_one_of("options.mode", &["summary", "detailed"], mode);
        };

    if !JSValue::from_bits(execution.to_bits()).is_undefined()
        && !string_value_is(execution, "default")
        && !string_value_is(execution, "eager")
    {
        throw_invalid_one_of("options.execution", &["default", "eager"], execution);
    }

    mode
}

fn emit_measure_memory_warning_once() {
    if MEASURE_MEMORY_WARNING_EMITTED.swap(true, Ordering::AcqRel) {
        return;
    }
    let scope = RuntimeHandleScope::new();
    let message = scope.root_nanbox_f64(string_value(
        "vm.measureMemory is an experimental feature and might change at any time",
    ));
    let warning_type = scope.root_nanbox_f64(string_value("ExperimentalWarning"));
    crate::process::js_process_emit_warning(
        message.get_nanbox_f64(),
        warning_type.get_nanbox_f64(),
        undefined_value(),
    );
}

fn set_object_field<'scope>(
    scope: &'scope RuntimeHandleScope,
    obj_handle: &RuntimeHandle<'scope>,
    name: &str,
    value: f64,
) {
    let value_handle = scope.root_nanbox_f64(value);
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let key_handle = scope.root_string_ptr(key);
    crate::object::js_object_set_field_by_name(
        obj_handle.get_raw_mut_ptr::<ObjectHeader>(),
        key_handle.get_raw_const_ptr::<StringHeader>() as *mut StringHeader,
        value_handle.get_nanbox_f64(),
    );
}

fn memory_entry<'scope>(scope: &'scope RuntimeHandleScope, estimate: f64, upper: f64) -> f64 {
    let range = crate::array::js_array_alloc(2);
    let range_handle = scope.root_raw_mut_ptr(range);
    let range = crate::array::js_array_push_f64(
        range_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>(),
        estimate,
    );
    range_handle.set_raw_mut_ptr::<crate::array::ArrayHeader>(range);
    let range = crate::array::js_array_push_f64(
        range_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>(),
        upper,
    );
    range_handle.set_raw_mut_ptr::<crate::array::ArrayHeader>(range);

    let entry = crate::object::js_object_alloc(0, 2);
    let entry_handle = scope.root_raw_mut_ptr(entry);
    set_object_field(scope, &entry_handle, "jsMemoryEstimate", estimate);
    set_object_field(
        scope,
        &entry_handle,
        "jsMemoryRange",
        boxed_ptr(range_handle.get_raw_const_ptr::<crate::array::ArrayHeader>() as *const u8),
    );
    boxed_ptr(entry_handle.get_raw_const_ptr::<ObjectHeader>() as *const u8)
}

fn wasm_memory_entry<'scope>(scope: &'scope RuntimeHandleScope) -> f64 {
    let entry = crate::object::js_object_alloc(0, 2);
    let entry_handle = scope.root_raw_mut_ptr(entry);
    set_object_field(scope, &entry_handle, "code", 0.0);
    set_object_field(scope, &entry_handle, "metadata", 0.0);
    boxed_ptr(entry_handle.get_raw_const_ptr::<ObjectHeader>() as *const u8)
}

fn measure_memory_result(mode: MeasureMemoryMode) -> f64 {
    let mut heap_used = 0_u64;
    let mut heap_total = 0_u64;
    crate::arena::js_arena_stats(&mut heap_used, &mut heap_total);
    let estimate = heap_used as f64;
    let upper = heap_total.max(heap_used) as f64;

    let scope = RuntimeHandleScope::new();
    let total = scope.root_nanbox_f64(memory_entry(&scope, estimate, upper));
    let wasm = scope.root_nanbox_f64(wasm_memory_entry(&scope));

    let result = crate::object::js_object_alloc(
        0,
        if mode == MeasureMemoryMode::Detailed {
            4
        } else {
            2
        },
    );
    let result_handle = scope.root_raw_mut_ptr(result);
    set_object_field(&scope, &result_handle, "total", total.get_nanbox_f64());
    set_object_field(&scope, &result_handle, "WebAssembly", wasm.get_nanbox_f64());

    if mode == MeasureMemoryMode::Detailed {
        let current = scope.root_nanbox_f64(memory_entry(&scope, estimate, upper));
        let other = crate::array::js_array_alloc(0);
        let other_handle = scope.root_raw_mut_ptr(other);
        set_object_field(&scope, &result_handle, "current", current.get_nanbox_f64());
        set_object_field(
            &scope,
            &result_handle,
            "other",
            boxed_ptr(other_handle.get_raw_const_ptr::<crate::array::ArrayHeader>() as *const u8),
        );
    }

    boxed_ptr(result_handle.get_raw_const_ptr::<ObjectHeader>() as *const u8)
}

fn resolved_promise(value: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    crate::value::js_nanbox_pointer(crate::promise::js_promise_resolved(
        value_handle.get_nanbox_f64(),
    ) as i64)
}

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

pub extern "C" fn js_vm_measure_memory(options: f64) -> f64 {
    emit_measure_memory_warning_once();
    let mode = validate_measure_memory_options(options);
    resolved_promise(measure_memory_result(mode))
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
