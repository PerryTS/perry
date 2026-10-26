//! Finalization jobs carry ownership identities, never their target wrapper.
//! They run outside GC and defer engine work while a component call is active.
use super::*;
use crate::object::owned_state_bag::{self as bag, COMPONENT_FINALIZATION};

pub(super) fn register(owner: f64, target: f64, request: Value) {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let target = scope.root_nanbox_f64(target);
    let registry = scope.root_nanbox_f64(undefined());
    let mut encoded = String::new();
    let result = crate::exception::catch_js_throw(|| {
        registry.set_nanbox_f64(unsafe {
            bag::load(
                object_pointer(owner.get_nanbox_f64()).unwrap() as usize,
                COMPONENT_FINALIZATION,
            )
        });
        if JSValue::from_bits(registry.get_nanbox_f64().to_bits()).is_undefined() {
            let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
                crate::fn_info!(collected, 1),
                1,
            ));
            crate::closure::js_closure_set_capture_f64(
                callback.get_raw_mut_ptr(),
                0,
                owner.get_nanbox_f64(),
            );
            registry.set_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::weakref::js_finreg_new(crate::value::js_nanbox_pointer(
                    callback.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
                )) as i64,
            ));
            unsafe {
                bag::store(
                    object_pointer(owner.get_nanbox_f64()).unwrap() as usize,
                    COMPONENT_FINALIZATION,
                    registry.get_nanbox_f64(),
                );
            }
        }
        encoded = serde_json::to_string(&request).unwrap();
        let held = scope.root_nanbox_f64(crate::value::js_nanbox_string(
            crate::string::js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32) as i64,
        ));
        // The unregister token is weak. Neither holdings nor the callback may
        // capture target, or the resource could never become unreachable.
        crate::weakref::js_finreg_register(
            registry.get_nanbox_f64(),
            target.get_nanbox_f64(),
            held.get_nanbox_f64(),
            target.get_nanbox_f64(),
        );
        undefined()
    });
    drop((request, encoded));
    super::finish(result);
}

pub(super) fn unregister(owner: f64, target: f64) {
    let registry = unsafe {
        bag::load(
            object_pointer(owner).unwrap() as usize,
            COMPONENT_FINALIZATION,
        )
    };
    if !JSValue::from_bits(registry.to_bits()).is_undefined() {
        crate::weakref::js_finreg_unregister(registry, target);
    }
}

extern "C" fn collected(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    held: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    let held = scope.root_nanbox_f64(held);
    let mut encoded = String::new();
    let mut request = Value::Null;
    let result = crate::exception::catch_js_throw(|| {
        encoded = string(held.get_nanbox_f64());
        request = serde_json::from_str(&encoded)
            .unwrap_or_else(|_| fail("Invalid component cleanup identity"));
        let context = context::get(owner.get_nanbox_f64());
        if unsafe { (*context).alive } {
            unsafe {
                (*context)
                    .pending_collections
                    .push(std::mem::take(&mut request));
            }
            drain(owner.get_nanbox_f64());
        }
        undefined()
    });
    drop((encoded, request));
    super::finish(result)
}

pub(super) fn drain(owner: f64) {
    if persistent::enabled(owner) {
        return persistent::collections::drain_collections(owner);
    }
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let first_error = scope.root_nanbox_f64(undefined());
    let context = context::get(owner.get_nanbox_f64());
    if unsafe { !(*context).alive || (*context).host.is_null() } {
        return;
    }
    let mut pending;
    let mut encoded = Vec::new();
    let mut output = Vec::new();
    let mut failed = false;
    loop {
        pending = unsafe { std::mem::take(&mut (*context).pending_collections) };
        if pending.is_empty() {
            break;
        }
        let mut deferred = false;
        for request in &pending {
            if !unsafe { (*context).alive } {
                break;
            }
            let frame = callback::Frame::new(&scope, owner.get_nanbox_f64());
            let result = crate::exception::catch_js_throw(|| {
                encoded = serde_json::to_vec(request).unwrap();
                let mut buffer = Buffer::default();
                let status = unsafe {
                    (host::ops().perry_component_host_resource_collect)(
                        (*context).host,
                        encoded.as_ptr(),
                        encoded.len(),
                        frame.token(),
                        &mut buffer,
                    )
                };
                output = unsafe { take(buffer) };
                if status == perry_abi::component_host::DEFERRED {
                    deferred = true;
                    unsafe {
                        (*context).pending_collections.push(request.clone());
                    }
                    return;
                }
                frame.rethrow();
                if status != perry_abi::component_host::OK {
                    fail(&String::from_utf8_lossy(&output));
                }
                // An unmaterialized host resource has no engine entry. Its
                // payload is released only after the same busy check succeeds.
                host_resource::payload::collect_local(owner.get_nanbox_f64(), request);
            });
            if let Err(error) = result {
                if !failed {
                    first_error.set_nanbox_f64(error);
                    failed = true;
                }
            }
        }
        if deferred || !unsafe { (*context).alive } {
            break;
        }
    }
    drop((pending, encoded, output));
    if failed {
        crate::exception::js_throw(first_error.get_nanbox_f64());
    }
}

/// A primary call failure takes precedence over a cleanup failure. Root either
/// returned value before destructors can allocate or trigger another collection.
pub(super) fn finish_call(owner: f64, result: Result<f64, f64>) -> f64 {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let failed = result.is_err();
    let value = scope.root_nanbox_f64(result.unwrap_or_else(|error| error));
    let cleanup = crate::exception::catch_js_throw(|| drain(owner.get_nanbox_f64()));
    if failed {
        crate::exception::js_throw(value.get_nanbox_f64());
    }
    if let Err(error) = cleanup {
        crate::exception::js_throw(error);
    }
    value.get_nanbox_f64()
}

/// A ready native result can be refused by a before hook or decoder before
/// JavaScript receives it. Retire its owned capabilities without manufacturing
/// wrapper objects; stale duplicate finalizer requests are safe to retry.
pub(super) fn returned(owner: f64, value: &Value) {
    fn gather(value: &Value, requests: &mut Vec<Value>) {
        match value {
            Value::Array(values) => {
                for value in values {
                    gather(value, requests)
                }
            }
            Value::Object(object) => {
                if value["owned"] == true {
                    if let Some(token) = value["$resource"].as_str() {
                        requests.push(serde_json::json!({"kind":"guest","token":token}));
                        return;
                    }
                    if value["$hostResource"].is_string() {
                        requests.push(serde_json::json!({"kind":"host","resource":{"token":value["$hostResource"],"type":value["type"],"generation":value["generation"]}}));
                        return;
                    }
                }
                if value["$future"].is_string() || value["$stream"].is_string() {
                    requests.push(serde_json::json!({"kind":"async-value","value":value}));
                    return;
                }
                for value in object.values() {
                    gather(value, requests);
                }
            }
            _ => {}
        }
    }
    let context = context::get(owner);
    if unsafe { (*context).alive } {
        unsafe { gather(value, &mut (*context).pending_collections) }
    }
}
