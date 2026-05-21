//! Promise-await FFIs: `js_await_js_promise` (legacy blocking path)
//! and `js_await_any_promise` (unified pump-driven adapter).

use super::*;

use deno_core::v8;

/// Await a V8 JavaScript Promise that was returned as a JS handle.
/// Takes a NaN-boxed f64 containing a JS handle to a V8 Promise.
/// Legacy/debug path: when explicitly enabled, runs the V8 event loop until
/// the Promise settles, then returns the resolved value.
/// If the value is not a Promise, returns it as-is.
/// Returns the resolved value as NaN-boxed f64.
#[no_mangle]
pub extern "C" fn js_await_js_promise(value: f64) -> f64 {
    jsruntime_profile_register();
    bump_v8_entry(V8EntryKind::LegacyBlockingAwait);
    if std::env::var_os("PERRY_JSRUNTIME_ENABLE_LEGACY_BLOCKING_AWAIT").is_none() {
        return js_await_any_promise(value);
    }
    bump_jsruntime(&JSRUNTIME_LEGACY_BLOCKING_AWAITS);
    let handle_id = match get_handle_id(value) {
        Some(id) => id,
        None => {
            return value;
        }
    };

    let tokio_rt = get_tokio_runtime();
    tokio_rt.block_on(async {
        JS_RUNTIME.with(|cell| {
            let mut opt = cell.borrow_mut();
            let state = match opt.as_mut() {
                Some(s) => s,
                None => {
                    return f64::from_bits(0x7FFC_0000_0000_0001);
                }
            };

            // Check if the value is a Promise and if it's already settled
            {
                deno_core::scope!(scope, &mut state.runtime);
                let v8_val = match get_js_handle(scope, handle_id) {
                    Some(v) => v,
                    None => {
                        return f64::from_bits(0x7FFC_0000_0000_0001);
                    }
                };

                if !v8_val.is_promise() {
                    return v8_to_native(scope, v8_val);
                }

                let promise = v8::Local::<v8::Promise>::try_from(v8_val).unwrap();
                let state_val = promise.state();
                if state_val != v8::PromiseState::Pending {
                    let result = promise.result(scope);
                    return v8_to_native(scope, result);
                }
            }

            // Promise is pending - run the event loop to settle it
            tokio::task::block_in_place(|| {
                let local_rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to create local Tokio runtime for V8 event loop");
                local_rt.block_on(async {
                    let _ = state.runtime.run_event_loop(Default::default()).await;
                })
            });

            // Now get the resolved value
            deno_core::scope!(scope, &mut state.runtime);
            let v8_val = match get_js_handle(scope, handle_id) {
                Some(v) => v,
                None => {
                    return f64::from_bits(0x7FFC_0000_0000_0001);
                }
            };

            if v8_val.is_promise() {
                let promise = v8::Local::<v8::Promise>::try_from(v8_val).unwrap();
                match promise.state() {
                    v8::PromiseState::Fulfilled => {
                        let result = promise.result(scope);
                        v8_to_native(scope, result)
                    }
                    v8::PromiseState::Rejected => {
                        f64::from_bits(0x7FFC_0000_0000_0001) // undefined
                    }
                    v8::PromiseState::Pending => {
                        f64::from_bits(0x7FFC_0000_0000_0001) // undefined
                    }
                }
            } else {
                v8_to_native(scope, v8_val)
            }
        })
    })
}

/// Await any promise — handles both JS handle promises (JS_HANDLE_TAG) and
/// native POINTER_TAG promises. If the value is neither, returns it as-is.
///
/// This is the unified await for F64 values where the type isn't known at compile time
/// (e.g., generic method dispatch returning either JS or native promises).
#[no_mangle]
pub extern "C" fn js_await_any_promise(value: f64) -> f64 {
    jsruntime_profile_register();
    let bits = value.to_bits();
    let tag = bits >> 48;

    if tag == 0x7FFB {
        // JS_HANDLE_TAG: if the handle is a V8 Promise, create a native
        // pending Promise and let the jsruntime pump settle it. This keeps
        // V8 promise progress inside Perry's existing event pump instead of
        // blocking inside await lowering.
        let handle_id = match get_handle_id(value) {
            Some(id) => id,
            None => return value,
        };

        let adapter_handle_id = with_runtime(|state| {
            deno_core::scope!(scope, &mut state.runtime);
            get_js_handle(scope, handle_id).and_then(|v| {
                if !v.is_promise() {
                    return None;
                }
                let promise = v8::Local::<v8::Promise>::try_from(v).unwrap();
                promise.mark_as_handled();
                Some(store_js_handle(scope, v))
            })
        });

        let Some(adapter_handle_id) = adapter_handle_id else {
            return value;
        };

        let native_promise = perry_runtime::promise::js_promise_new();
        FOREIGN_PROMISE_ADAPTERS.with(|adapters| {
            adapters.borrow_mut().push(ForeignPromiseAdapter {
                handle_id: adapter_handle_id,
                native_promise,
            });
        });
        bump_v8_entry(V8EntryKind::ForeignPromiseAdapter);
        bump_jsruntime(&JSRUNTIME_ADAPTERS_CREATED);
        perry_runtime::event_pump::js_notify_main_thread();
        return boxed_native_promise(native_promise);
    }

    // For POINTER_TAG (native promises) and all other values, return as-is.
    // The codegen-emitted busy-wait loop handles native promise polling correctly
    // using the same thread's microtask queue.
    value
}
