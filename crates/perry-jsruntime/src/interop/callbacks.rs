//! Native-callback registration and the V8 trampoline that dispatches
//! into Perry closures (`js_create_callback` +
//! `native_callback_trampoline`).

use super::*;

use deno_core::v8;

// Storage for native callback function pointers and their closure environments
thread_local! {
    pub(crate) static NATIVE_CALLBACKS: std::cell::RefCell<std::collections::HashMap<u64, (i64, i64)>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
    pub(crate) static NEXT_CALLBACK_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

/// Create a V8 function that wraps a native callback
/// func_ptr: Pointer to the native function to call
/// closure_env: Pointer to the closure environment (or 0 for no environment)
/// param_count: Number of parameters the callback expects
/// Returns a JS handle to the V8 function
#[no_mangle]
pub unsafe extern "C" fn js_create_callback(
    func_ptr: i64,
    closure_env: i64,
    param_count: i64,
) -> f64 {
    bump_v8_entry(V8EntryKind::CallbackCreate);
    // Store the callback info
    let callback_id = NEXT_CALLBACK_ID.with(|id| {
        let current = id.get();
        id.set(current + 1);
        current
    });

    NATIVE_CALLBACKS.with(|callbacks| {
        callbacks
            .borrow_mut()
            .insert(callback_id, (func_ptr, closure_env));
    });

    with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);

        // Create external data to store the callback ID and param count
        let data_array = v8::Array::new(scope, 2);
        let id_val = v8::Number::new(scope, callback_id as f64);
        let count_val = v8::Number::new(scope, param_count as f64);
        data_array.set_index(scope, 0, id_val.into());
        data_array.set_index(scope, 1, count_val.into());

        // Create the callback function
        let callback_fn = v8::Function::builder(native_callback_trampoline)
            .data(data_array.into())
            .build(scope);

        match callback_fn {
            Some(func) => {
                let handle_id = store_js_handle(scope, func.into());
                make_js_handle_value(handle_id)
            }
            None => {
                log::error!("Failed to create callback function");
                f64::from_bits(0x7FFC_0000_0000_0001)
            }
        }
    })
}

/// Trampoline function that V8 calls when a native callback is invoked
pub(crate) fn native_callback_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    bump_v8_entry(V8EntryKind::CallbackInvoke);
    // Get the callback ID and param count from the data
    let data = args.data();
    if !data.is_array() {
        retval.set(v8::undefined(scope).into());
        return;
    }

    let data_array = v8::Local::<v8::Array>::try_from(data).unwrap();
    let callback_id = data_array
        .get_index(scope, 0)
        .and_then(|v| v.number_value(scope))
        .unwrap_or(0.0) as u64;
    let _param_count = data_array
        .get_index(scope, 1)
        .and_then(|v| v.number_value(scope))
        .unwrap_or(0.0) as i64;

    // Get the function pointer and closure environment
    let (func_ptr, closure_env) = NATIVE_CALLBACKS.with(|callbacks| {
        callbacks
            .borrow()
            .get(&callback_id)
            .copied()
            .unwrap_or((0, 0))
    });

    if func_ptr == 0 {
        log::error!("Native callback not found: {}", callback_id);
        retval.set(v8::undefined(scope).into());
        return;
    }

    // Convert arguments to native format
    let arg_count = args.length();
    let mut native_args: Vec<f64> = Vec::with_capacity(arg_count as usize);
    for i in 0..arg_count {
        let arg = args.get(i);
        native_args.push(v8_to_native(scope, arg));
    }

    // Issue #255: stash this scope so re-entrant FFIs (e.g. js_get_property
    // called from inside the Perry callback to read `ctx.deltaTime`) can
    // reuse it instead of calling state.runtime.handle_scope() — which
    // V8's scope tracking rejects with "active scope can't be dropped"
    // because we'd be creating a new scope above the one V8 itself has
    // active for this trampoline call. Guard auto-restores any prior
    // stashed scope on Drop, so nested trampoline invocations work.
    let _scope_guard = crate::stash_trampoline_scope(scope);

    // Call the native function
    // Function signature: fn(closure_env: i64, args_ptr: *const f64, args_len: i64) -> f64
    type CallbackFn = extern "C" fn(i64, *const f64, i64) -> f64;
    let callback: CallbackFn = unsafe { std::mem::transmute(func_ptr as *const ()) };
    let result = callback(closure_env, native_args.as_ptr(), native_args.len() as i64);

    // Convert result back to V8
    let v8_result = native_to_v8(scope, result);
    retval.set(v8_result);
}
