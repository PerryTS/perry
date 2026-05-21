//! `new`-instance FFI: `js_new_instance` (by class name on a module),
//! `js_new_from_handle` (by JS-handle to a constructor), and the
//! `js_new_from_handle_v8_impl` callback perry-runtime hits when a
//! native `new` falls through to a JS handle constructor.

use super::*;

use deno_core::v8;
use std::ffi::{c_char, CStr};

/// V8 new_instance implementation — called via callback from perry-runtime's js_new_from_handle
/// when the constructor is a JS handle (JS_HANDLE_TAG).
pub(crate) unsafe extern "C" fn js_new_from_handle_v8_impl(
    constructor_handle: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    bump_v8_entry(V8EntryKind::HandleConstructor);
    let args = if args_ptr.is_null() || args_len == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(args_ptr, args_len).to_vec()
    };

    with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);

        let constructor_val = native_to_v8(scope, constructor_handle);
        if !constructor_val.is_function() {
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }

        let constructor = v8::Local::<v8::Function>::try_from(constructor_val).unwrap();

        let v8_args: Vec<v8::Local<v8::Value>> = args
            .iter()
            .map(|&arg| {
                let fixed = fixup_native_for_v8(arg);
                native_to_v8(scope, fixed)
            })
            .collect();

        v8::tc_scope!(tc_scope, scope);
        match constructor.new_instance(tc_scope, &v8_args) {
            Some(r) => v8_to_native(tc_scope, r.into()),
            None => {
                if let Some(exception) = tc_scope.exception() {
                    let msg = exception.to_rust_string_lossy(tc_scope);
                    eprintln!("[js_new_from_handle_v8] constructor failed: {}", msg);
                }
                f64::from_bits(0x7FFC_0000_0000_0001)
            }
        }
    })
}

/// Create a new instance of a JavaScript class
/// module_handle: Handle to the loaded module
/// class_name: Name of the class to instantiate
/// args: Array of NaN-boxed f64 arguments
/// Returns a JS handle to the new instance
#[no_mangle]
pub unsafe extern "C" fn js_new_instance(
    module_handle: u64,
    class_name_ptr: *const i8,
    class_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    bump_v8_entry(V8EntryKind::NewInstance);
    let name_slice = if class_name_ptr.is_null() {
        return f64::from_bits(0x7FFC_0000_0000_0001); // undefined
    } else if class_name_len > 0 {
        std::slice::from_raw_parts(class_name_ptr as *const u8, class_name_len)
    } else {
        CStr::from_ptr(class_name_ptr as *const c_char).to_bytes()
    };

    let class_name = match std::str::from_utf8(name_slice) {
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

        deno_core::scope!(scope, &mut state.runtime);
        let namespace = v8::Local::new(scope, namespace);

        // Get the class constructor from the namespace
        let key = match v8::String::new(scope, class_name) {
            Some(k) => k,
            None => return f64::from_bits(0x7FFC_0000_0000_0001),
        };

        let constructor_val = match namespace.get(scope, key.into()) {
            Some(v) => v,
            None => {
                log::error!("Class '{}' not found in module", class_name);
                return f64::from_bits(0x7FFC_0000_0000_0001);
            }
        };

        if !constructor_val.is_function() {
            log::error!("'{}' is not a constructor", class_name);
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }

        let constructor = v8::Local::<v8::Function>::try_from(constructor_val).unwrap();

        // Convert arguments from native to V8
        let v8_args: Vec<v8::Local<v8::Value>> = args
            .iter()
            .map(|&arg| native_to_v8(scope, fixup_native_for_v8(arg)))
            .collect();

        // Call the constructor with 'new'
        let result = match constructor.new_instance(scope, &v8_args) {
            Some(r) => r,
            None => {
                log::error!("Constructor call failed");
                return f64::from_bits(0x7FFC_0000_0000_0001);
            }
        };

        v8_to_native(scope, result.into())
    })
}

/// Create a new instance using a JS handle to a constructor function
/// constructor_handle: NaN-boxed value containing a JS handle to a constructor
/// args: Array of NaN-boxed f64 arguments
/// Returns a JS handle to the new instance
#[no_mangle]
pub unsafe extern "C" fn js_new_from_handle(
    constructor_handle: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    bump_v8_entry(V8EntryKind::NewFromHandle);
    let ctor_bits = constructor_handle.to_bits();
    let tag = ctor_bits >> 48;

    // Only process JS handles — for non-handle constructors, return undefined
    if tag != 0x7FFB {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    }

    let args = if args_ptr.is_null() || args_len == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(args_ptr, args_len).to_vec()
    };

    with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);

        // Get the constructor from the handle
        let constructor_val = native_to_v8(scope, constructor_handle);
        if !constructor_val.is_function() {
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }

        let constructor = v8::Local::<v8::Function>::try_from(constructor_val).unwrap();

        // Convert arguments from native to V8
        let v8_args: Vec<v8::Local<v8::Value>> = args
            .iter()
            .map(|&arg| {
                let fixed = fixup_native_for_v8(arg);
                native_to_v8(scope, fixed)
            })
            .collect();

        // Call the constructor with 'new'
        v8::tc_scope!(tc_scope, scope);
        match constructor.new_instance(tc_scope, &v8_args) {
            Some(r) => v8_to_native(tc_scope, r.into()),
            None => {
                if let Some(exception) = tc_scope.exception() {
                    let msg = exception.to_rust_string_lossy(tc_scope);
                    eprintln!("[js_new_from_handle] constructor failed: {}", msg);
                }
                f64::from_bits(0x7FFC_0000_0000_0001)
            }
        }
    })
}
