//! Function / method / value-call FFI: `js_call_function`,
//! `js_call_v8_export`, `js_call_v8_member_method`, `js_call_method`,
//! `js_call_value`, and the `js_register_native_function` stub.

use super::*;

use deno_core::v8;
use std::ffi::{c_char, CStr};

/// Convert a NaN-boxed f64 to a V8 value, returning None if the conversion fails
/// This is specifically for cases where we need to handle the error explicitly
pub(crate) fn nanbox_to_v8<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: f64,
) -> Option<v8::Local<'s, v8::Value>> {
    // Check if it's a JS handle first
    if is_js_handle(value) {
        if let Some(handle_id) = get_handle_id(value) {
            return get_js_handle(scope, handle_id);
        }
        return None;
    }
    // Use the standard conversion for other values
    Some(native_to_v8(scope, value))
}

/// Call a JavaScript function with arguments
/// Returns the result as a NaN-boxed f64
#[no_mangle]
pub unsafe extern "C" fn js_call_function(
    module_handle: u64,
    func_name_ptr: *const i8,
    func_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    bump_v8_entry(V8EntryKind::FunctionCall);
    let name_slice = if func_name_ptr.is_null() {
        return f64::from_bits(0x7FFC_0000_0000_0001); // undefined
    } else if func_name_len > 0 {
        std::slice::from_raw_parts(func_name_ptr as *const u8, func_name_len)
    } else {
        CStr::from_ptr(func_name_ptr as *const c_char).to_bytes()
    };

    let func_name = match std::str::from_utf8(name_slice) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(0x7FFC_0000_0000_0001),
    };

    let args = if args_ptr.is_null() || args_len == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(args_ptr, args_len).to_vec()
    };

    with_runtime(|state| {
        let module_id = module_handle as deno_core::ModuleId;
        let namespace = match state.runtime.get_module_namespace(module_id) {
            Ok(ns) => ns,
            Err(e) => {
                log::error!("Failed to get module namespace: {}", e);
                return f64::from_bits(0x7FFC_0000_0000_0001);
            }
        };

        call_function_impl(state, namespace, func_name, &args)
    })
}

/// Issue #678: invoke a named export of a V8-fallback module by specifier.
///
/// Bundles `js_load_module` + `js_call_function` into a single FFI entry the
/// codegen can drop in wherever an import resolves to a `ModuleKind::Interpreted`
/// module. Without this, the codegen would emit `perry_fn_<src>__<name>` for
/// imports out of a V8-routed module — but no such native symbol exists, so
/// the linker fails with `Undefined symbols: _perry_fn_..._<name>`.
///
/// `specifier_ptr` / `specifier_len` and `export_name_ptr` / `export_name_len`
/// follow the same ptr+len convention as `js_load_module` / `js_call_function`
/// (zero len = null-terminated C string). `args_ptr` / `args_len` carry the
/// already-NaN-boxed Perry argument doubles; result is also NaN-boxed.
#[no_mangle]
pub unsafe extern "C" fn js_call_v8_export(
    specifier_ptr: *const i8,
    specifier_len: usize,
    export_name_ptr: *const i8,
    export_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    bump_v8_entry(V8EntryKind::V8ExportCall);
    let module_handle = js_load_module(specifier_ptr, specifier_len);
    if module_handle == 0 {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    }
    js_call_function(
        module_handle,
        export_name_ptr,
        export_name_len,
        args_ptr,
        args_len,
    )
}

/// Issue #818 (Effect.succeed pattern): invoke a method on a NAMED member of a
/// V8-fallback module — `Effect.succeed(42)` where `Effect` is imported by name
/// (`import { Effect } from 'effect'`) and the export is itself a sub-namespace
/// object that holds the actual `succeed` function.
///
/// Without this entry, `StaticMethodCall { class_name: "Effect", method_name:
/// "succeed" }` fell through to `double_literal(0.0)` because:
///   - `methods.get(("Effect","succeed"))` misses (Effect isn't a perry class)
///   - `namespace_imports.contains("Effect")` is false (it's a Named, not a
///     `import * as Effect`)
///   - The existing `js_call_v8_export` would call `effect.succeed(...)` at
///     the top level of the module, but the actual function lives at
///     `effect.Effect.succeed`.
///
/// Bundles `js_load_module` + namespace-property-get + method-call so the
/// codegen can drop in a single FFI call wherever a named V8 import is invoked
/// as a static method. Argument and return marshalling follows the same
/// conventions as `js_call_v8_export` — args already NaN-boxed, result
/// NaN-boxed (objects come back as JS handles so subsequent `.value` /
/// `.pipe()` accesses route through the existing HANDLE_PROPERTY / METHOD
/// dispatch and reach V8 again with the prototype intact).
#[no_mangle]
pub unsafe extern "C" fn js_call_v8_member_method(
    specifier_ptr: *const i8,
    specifier_len: usize,
    member_name_ptr: *const i8,
    member_name_len: usize,
    method_name_ptr: *const i8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    bump_v8_entry(V8EntryKind::V8ExportCall);
    let module_handle = js_load_module(specifier_ptr, specifier_len);
    if module_handle == 0 {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    }
    let member_name = match c_str_to_utf8(member_name_ptr, member_name_len) {
        Some(s) => s,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };
    let method_name = match c_str_to_utf8(method_name_ptr, method_name_len) {
        Some(s) => s,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };
    let args = if args_ptr.is_null() || args_len == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(args_ptr, args_len).to_vec()
    };

    with_runtime(|state| {
        let module_id = module_handle as deno_core::ModuleId;
        let namespace = match state.runtime.get_module_namespace(module_id) {
            Ok(ns) => ns,
            Err(e) => {
                log::error!("Failed to get module namespace: {}", e);
                return f64::from_bits(0x7FFC_0000_0000_0001);
            }
        };

        deno_core::scope!(scope, &mut state.runtime);
        let namespace = v8::Local::new(scope, namespace);
        v8::tc_scope!(tc_scope, scope);

        // Walk the member chain (single hop here — caller passes `Effect`
        // for `Effect.succeed(args)`). Result must be a callable host.
        let member_key = match v8::String::new(tc_scope, &member_name) {
            Some(k) => k,
            None => return f64::from_bits(0x7FFC_0000_0000_0001),
        };
        let member_val = match namespace.get(tc_scope, member_key.into()) {
            Some(v) => v,
            None => {
                eprintln!(
                    "[JS-INTEROP] V8 member '{}' not found on module namespace",
                    member_name
                );
                return f64::from_bits(0x7FFC_0000_0000_0001);
            }
        };
        if !member_val.is_object() {
            eprintln!(
                "[JS-INTEROP] V8 member '{}' is not an object (got typeof {})",
                member_name,
                if member_val.is_function() {
                    "function"
                } else {
                    "primitive"
                }
            );
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }
        let member_obj = match member_val.to_object(tc_scope) {
            Some(o) => o,
            None => return f64::from_bits(0x7FFC_0000_0000_0001),
        };

        let method_key = match v8::String::new(tc_scope, &method_name) {
            Some(k) => k,
            None => return f64::from_bits(0x7FFC_0000_0000_0001),
        };
        let method_val = match member_obj.get(tc_scope, method_key.into()) {
            Some(v) => v,
            None => {
                eprintln!(
                    "[JS-INTEROP] V8 method '{}.{}' not found",
                    member_name, method_name
                );
                return f64::from_bits(0x7FFC_0000_0000_0001);
            }
        };
        if !method_val.is_function() {
            eprintln!(
                "[JS-INTEROP] V8 '{}.{}' is not a function",
                member_name, method_name
            );
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }
        let method = v8::Local::<v8::Function>::try_from(method_val).unwrap();

        let v8_args: Vec<v8::Local<v8::Value>> = args
            .iter()
            .map(|&a| native_to_v8(tc_scope, fixup_native_for_v8(a)))
            .collect();

        // Bind `this` to the member object so methods that use `this`
        // (most class-style static methods) see the right receiver.
        let result = match method.call(tc_scope, member_obj.into(), &v8_args) {
            Some(r) => r,
            None => {
                if tc_scope.has_caught() {
                    if let Some(exception) = tc_scope.exception() {
                        let msg = exception.to_rust_string_lossy(tc_scope);
                        eprintln!(
                            "[JS-INTEROP] '{}.{}' threw: {}",
                            member_name, method_name, msg
                        );
                    }
                }
                return f64::from_bits(0x7FFC_0000_0000_0001);
            }
        };

        v8_to_native(tc_scope, result)
    })
}

fn call_function_impl(
    state: &mut JsRuntimeState,
    namespace: v8::Global<v8::Object>,
    func_name: &str,
    args: &[f64],
) -> f64 {
    deno_core::scope!(scope, &mut state.runtime);
    let namespace = v8::Local::new(scope, namespace);

    // Use TryCatch to properly handle V8 exceptions
    v8::tc_scope!(tc_scope, scope);

    // Get the function from the namespace
    let key = match v8::String::new(tc_scope, func_name) {
        Some(k) => k,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };

    let func_val = match namespace.get(tc_scope, key.into()) {
        Some(v) => v,
        None => {
            log::error!("Function '{}' not found in module", func_name);
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }
    };

    if !func_val.is_function() {
        log::error!("'{}' is not a function", func_name);
        return f64::from_bits(0x7FFC_0000_0000_0001);
    }

    let func = v8::Local::<v8::Function>::try_from(func_val).unwrap();

    // Convert arguments from native to V8
    let v8_args: Vec<v8::Local<v8::Value>> = args
        .iter()
        .map(|&arg| native_to_v8(tc_scope, fixup_native_for_v8(arg)))
        .collect();

    // Call the function
    let undefined = v8::undefined(tc_scope);
    let result = match func.call(tc_scope, undefined.into(), &v8_args) {
        Some(r) => r,
        None => {
            // Get and log the exception, then clear it so subsequent calls work
            if tc_scope.has_caught() {
                if let Some(exception) = tc_scope.exception() {
                    // Try to get detailed message
                    if let Some(msg_obj) = tc_scope.message() {
                        let msg_str = msg_obj.get(tc_scope).to_rust_string_lossy(tc_scope);
                        let line = msg_obj.get_line_number(tc_scope).unwrap_or(0);
                        let script = msg_obj
                            .get_script_resource_name(tc_scope)
                            .map(|s| s.to_rust_string_lossy(tc_scope))
                            .unwrap_or_default();
                        eprintln!(
                            "[JS-INTEROP] Function '{}' threw: {} ({}:{})",
                            func_name, msg_str, script, line
                        );
                    } else {
                        let msg = exception.to_rust_string_lossy(tc_scope);
                        eprintln!("[JS-INTEROP] Function '{}' threw: {}", func_name, msg);
                    }

                    // Log args for debugging
                    for (i, &arg) in args.iter().enumerate() {
                        let bits = arg.to_bits();
                        let tag = bits >> 48;
                        eprintln!(
                            "[JS-INTEROP]   arg[{}]: bits=0x{:016x} tag=0x{:04x}",
                            i, bits, tag
                        );
                    }
                }
                // Exception is automatically cleared when TryCatch scope drops
            } else {
                eprintln!(
                    "[JS-INTEROP] Function '{}' call returned None (no exception)",
                    func_name
                );
            }
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }
    };

    // Handle promises - for now just return the promise object
    // Proper async support would require more complex handling
    v8_to_native(tc_scope, result)
}

/// Call a method on a JavaScript object
#[no_mangle]
pub unsafe extern "C" fn js_call_method(
    object_ptr: f64,
    method_name_ptr: *const i8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    bump_v8_entry(V8EntryKind::MethodCall);
    let name_slice = if method_name_ptr.is_null() {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    } else if method_name_len > 0 {
        std::slice::from_raw_parts(method_name_ptr as *const u8, method_name_len)
    } else {
        CStr::from_ptr(method_name_ptr as *const c_char).to_bytes()
    };

    let method_name = match std::str::from_utf8(name_slice) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(0x7FFC_0000_0000_0001),
    };

    let args = if args_ptr.is_null() || args_len == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(args_ptr, args_len).to_vec()
    };

    with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);

        // Convert the object pointer to a V8 object
        let obj_val = native_to_v8(scope, object_ptr);
        if !obj_val.is_object() {
            log::error!("Value is not an object");
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }

        let obj = obj_val.to_object(scope).unwrap();

        // Get the method
        let key = match v8::String::new(scope, method_name) {
            Some(k) => k,
            None => return f64::from_bits(0x7FFC_0000_0000_0001),
        };

        let method_val = match obj.get(scope, key.into()) {
            Some(v) => v,
            None => {
                log::error!("Method '{}' not found on object", method_name);
                return f64::from_bits(0x7FFC_0000_0000_0001);
            }
        };

        if !method_val.is_function() {
            log::error!("'{}' is not a function", method_name);
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }

        let method = v8::Local::<v8::Function>::try_from(method_val).unwrap();

        // Convert arguments
        let v8_args: Vec<v8::Local<v8::Value>> = args
            .iter()
            .map(|&arg| native_to_v8(scope, fixup_native_for_v8(arg)))
            .collect();

        // Call with 'this' bound to the object
        let result = match method.call(scope, obj.into(), &v8_args) {
            Some(r) => r,
            None => {
                return f64::from_bits(0x7FFC_0000_0000_0001);
            }
        };

        v8_to_native(scope, result)
    })
}

/// Call a JavaScript function value directly (for callback parameters)
/// func_value: NaN-boxed f64 containing a V8 function handle
/// args_ptr: pointer to array of f64 arguments
/// args_len: number of arguments
/// Returns the result as a NaN-boxed f64
#[no_mangle]
pub unsafe extern "C" fn js_call_value(
    func_value: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    bump_v8_entry(V8EntryKind::ValueCall);
    let args = if args_ptr.is_null() || args_len == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(args_ptr, args_len).to_vec()
    };

    with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);
        v8::tc_scope!(tc_scope, scope);

        // Extract the function from the NaN-boxed value
        let func_local = match nanbox_to_v8(tc_scope, func_value) {
            Some(v) => v,
            None => {
                log::error!("Failed to convert function value from NaN-boxed");
                return f64::from_bits(0x7FFC_0000_0000_0001); // undefined
            }
        };

        if !func_local.is_function() {
            log::error!("Value is not a function");
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }

        let func = v8::Local::<v8::Function>::try_from(func_local).unwrap();

        // Convert arguments
        let v8_args: Vec<v8::Local<v8::Value>> = args
            .iter()
            .map(|&arg| native_to_v8(tc_scope, fixup_native_for_v8(arg)))
            .collect();

        // Call with undefined as 'this'
        let undefined = v8::undefined(tc_scope);
        let result = match func.call(tc_scope, undefined.into(), &v8_args) {
            Some(r) => r,
            None => {
                if tc_scope.has_caught() {
                    if let Some(msg_obj) = tc_scope.message() {
                        let msg_str = msg_obj.get(tc_scope).to_rust_string_lossy(tc_scope);
                        let line = msg_obj.get_line_number(tc_scope).unwrap_or(0);
                        let script = msg_obj
                            .get_script_resource_name(tc_scope)
                            .map(|s| s.to_rust_string_lossy(tc_scope))
                            .unwrap_or_default();
                        log::error!(
                            "[JS-INTEROP] Function value threw: {} ({}:{})",
                            msg_str,
                            script,
                            line
                        );
                    } else if let Some(exception) = tc_scope.exception() {
                        log::error!(
                            "[JS-INTEROP] Function value threw: {}",
                            exception.to_rust_string_lossy(tc_scope)
                        );
                    }
                } else {
                    log::error!("Function call failed");
                }
                return f64::from_bits(0x7FFC_0000_0000_0001);
            }
        };

        v8_to_native(tc_scope, result)
    })
}

/// Register a native function that can be called from JavaScript
#[no_mangle]
pub unsafe extern "C" fn js_register_native_function(
    name_ptr: *const i8,
    name_len: usize,
    func_ptr: *const u8,
    param_count: usize,
) {
    bump_v8_entry(V8EntryKind::NativeFunctionRegister);
    let name_slice = if name_ptr.is_null() {
        return;
    } else if name_len > 0 {
        std::slice::from_raw_parts(name_ptr as *const u8, name_len)
    } else {
        CStr::from_ptr(name_ptr as *const c_char).to_bytes()
    };

    let _func_name = match std::str::from_utf8(name_slice) {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };

    // Store the function pointer and param count for later use
    log::debug!(
        "Registered native function at {:?} with {} params",
        func_ptr,
        param_count
    );

    // TODO: Implement proper native function registration
}
