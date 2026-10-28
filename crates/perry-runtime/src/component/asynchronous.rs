//! Promise-based native component calls run only on their owning agent.
pub(super) mod imports;
#[cfg(test)]
mod tests;
use super::*;
use crate::native_payload::{self as payload, NativePayloadFamily, OwnerLink};
use perry_abi::component_host::{ASYNC_PENDING, HTTP_FAILURE, OK, RELEASED_ERROR};
use std::cell::{Cell, RefCell};
static OPERATION: NativePayloadFamily = NativePayloadFamily {
    class_id: crate::native_class_ids::COMPONENT_CALL_OPERATION,
    links_owner: true,
    name: "ComponentCallOperation",
    constructor_export: None,
    constructor_length: 0,
    install_prototype: |_| {},
};
struct Operation {
    interface: String,
    function: String,
    args: Vec<u8>,
    ids: crate::async_hooks::AsyncResourceIds,
    http_request: *mut core::ffi::c_void,
    started: bool,
    failed: bool,
}
crate::perry_thread_local! {
    // Native links retain traced payload cells; this queue holds no JS address.
    static PENDING: RefCell<Vec<OwnerLink>> = const { RefCell::new(Vec::new()) };
    static PUMPING: Cell<bool> = const { Cell::new(false) };
}
unsafe extern "C" fn wake(agent: u64) {
    #[cfg(not(target_arch = "wasm32"))]
    crate::event_pump::notify_agent_work(agent);
    #[cfg(target_arch = "wasm32")]
    let _ = agent;
}
fn native<'a>(owner: f64) -> &'a mut Operation {
    unsafe { payload::payload_mut::<Operation>(owner, &OPERATION) }.expect("live component call")
}
fn remember(owner: f64, error: f64) {
    if native(owner).failed {
        return;
    }
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    payload::state_set(owner.get_nanbox_f64(), &OPERATION, b"error", error);
    native(owner.get_nanbox_f64()).failed = true;
}
fn retire(agent: crate::agent::AgentId) {
    if agent == crate::agent::current_agent() {
        PENDING.with(|q| q.borrow_mut().clear());
    }
}
extern "C" fn has_active() -> i32 {
    i32::from(PUMPING.with(Cell::get) || PENDING.with(|q| !q.borrow().is_empty()))
}
pub(super) fn submit(component: f64, interface: &str, function: &str, args: f64) -> f64 {
    submit_kind(
        component,
        interface,
        function,
        args,
        undefined(),
        std::ptr::null_mut(),
        undefined(),
    )
}
/// The traced HTTP owner retains this native accept handle until the call
/// settles. The child contains no borrowed JS pointer or callback frame.
pub(super) fn submit_http(
    component: f64,
    interface: &str,
    owner: f64,
    request: *mut core::ffi::c_void,
    promise: f64,
) -> f64 {
    assert!(!request.is_null());
    submit_kind(
        component,
        interface,
        "handle",
        undefined(),
        owner,
        request,
        promise,
    )
}
fn submit_kind(
    component: f64,
    interface: &str,
    function: &str,
    args: f64,
    http_owner: f64,
    http_request: *mut core::ffi::c_void,
    completion: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(component);
    let args = scope.root_nanbox_f64(args);
    let http_owner = scope.root_nanbox_f64(http_owner);
    let completion = scope.root_nanbox_f64(completion);
    let owner = scope.root_nanbox_f64(undefined());
    let promise = scope.root_raw_mut_ptr::<crate::promise::Promise>(std::ptr::null_mut());
    let retained = scope.root_nanbox_f64(undefined());
    let saved_context = scope.root_nanbox_f64(undefined());
    let mut encoded = Vec::new();
    let result = crate::exception::catch_js_throw(|| {
        retained.set_nanbox_f64(value::retention::new_plan());
        encoded = if http_request.is_null() {
            let world = unsafe { &(*context::get(component.get_nanbox_f64())).world };
            let signature = context::signature(world, "exports", interface, function);
            let params = signature["params"]
                .as_array()
                .unwrap_or_else(|| fail("Invalid WIT parameter manifest"));
            serde_json::to_vec(&value::typed::arguments(
                args.get_nanbox_f64(),
                params,
                world,
                component.get_nanbox_f64(),
                retained.get_nanbox_f64(),
            ))
            .unwrap()
        } else {
            b"[]".to_vec()
        };
        if http_request.is_null() {
            let captured = scope.root_raw_mut_ptr(crate::async_context::alloc_snapshot_holder(
                crate::async_context::capture_context(),
            ));
            promise.set_raw_mut_ptr(crate::promise::js_promise_new());
            crate::promise::set_promise_context_snapshot(promise.get_raw_mut_ptr(), unsafe {
                (*captured.get_raw_mut_ptr::<crate::async_context::AsyncSnapshotHolder>())
                    .snapshot
                    .clone()
            });
        } else {
            // HTTP's existing public Promise already owns its ALS snapshot.
            promise.set_raw_mut_ptr(
                JSValue::from_bits(completion.get_nanbox_f64().to_bits())
                    .as_pointer::<crate::promise::Promise>()
                    .cast_mut(),
            );
        }
        let (record, ids) = callback::saved::capture(crate::value::js_nanbox_pointer(
            promise.get_raw_mut_ptr::<crate::promise::Promise>() as i64,
        ));
        saved_context.set_nanbox_f64(record);
        owner.set_nanbox_f64(payload::alloc_closed(&OPERATION, &[]));
        payload::state_set(
            owner.get_nanbox_f64(),
            &OPERATION,
            b"context",
            saved_context.get_nanbox_f64(),
        );
        payload::state_set(
            owner.get_nanbox_f64(),
            &OPERATION,
            b"component",
            component.get_nanbox_f64(),
        );
        payload::state_set(
            owner.get_nanbox_f64(),
            &OPERATION,
            b"arguments",
            retained.get_nanbox_f64(),
        );
        payload::state_set(
            owner.get_nanbox_f64(),
            &OPERATION,
            b"promise",
            crate::value::js_nanbox_pointer(
                promise.get_raw_mut_ptr::<crate::promise::Promise>() as i64
            ),
        );
        if !http_request.is_null() {
            payload::state_set(
                owner.get_nanbox_f64(),
                &OPERATION,
                b"http-owner",
                http_owner.get_nanbox_f64(),
            );
        }
        let ctx = context::get(component.get_nanbox_f64());
        if unsafe { (*ctx).host.is_null() || !(*ctx).alive } {
            fail("Component is not initialized or has been disposed");
        }
        if unsafe { (*ctx).async_active } {
            fail("Component already has a pending async call");
        }
        let external_bytes = encoded
            .capacity()
            .saturating_add(interface.len())
            .saturating_add(function.len())
            .saturating_add(std::mem::size_of::<Operation>());
        payload::attach(
            owner.get_nanbox_f64(),
            &OPERATION,
            Operation {
                interface: interface.to_owned(),
                function: function.to_owned(),
                args: std::mem::take(&mut encoded),
                ids,
                http_request,
                started: false,
                failed: false,
            },
            external_bytes,
        )
        .unwrap_or_else(|_| fail("Cannot attach async component call"));
        unsafe { (*ctx).async_active = true };
        let link = payload::owner_link(owner.get_nanbox_f64(), &OPERATION).unwrap();
        unsafe { payload::link_ref(link) };
        crate::agent::register_retire_hook(retire);
        crate::stdlib_pump::js_register_agent_aux_pump(pump);
        crate::stdlib_pump::js_register_aux_has_active(has_active);
        PENDING.with(|q| q.borrow_mut().push(link));
        unsafe { wake(crate::agent::current_agent()) };
        crate::value::js_nanbox_pointer(promise.get_raw_mut_ptr::<crate::promise::Promise>() as i64)
    });
    if result.is_err() {
        payload::close(owner.get_nanbox_f64(), &OPERATION);
    }
    drop(encoded);
    finish(result)
}
fn step(owner: f64) -> bool {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let component = scope.root_nanbox_f64(payload::state_get(
        owner.get_nanbox_f64(),
        &OPERATION,
        b"component",
    ));
    let promise = scope.root_nanbox_f64(payload::state_get(
        owner.get_nanbox_f64(),
        &OPERATION,
        b"promise",
    ));
    let saved_context = scope.root_nanbox_f64(payload::state_get(
        owner.get_nanbox_f64(),
        &OPERATION,
        b"context",
    ));
    let ids = native(owner.get_nanbox_f64()).ids;
    let primary_batch = callback::FrameBatch::new(&scope, component.get_nanbox_f64());
    let cleanup_batch = callback::FrameBatch::new(&scope, component.get_nanbox_f64());
    let primary = primary_batch.for_context(1, saved_context.get_nanbox_f64(), ids);
    let cleanup = cleanup_batch.for_context(1, saved_context.get_nanbox_f64(), ids);
    let result_value = scope.root_nanbox_f64(undefined());
    let mut output = Vec::new();
    let mut parsed = Value::Null;
    let mut status = OK;
    let outcome = primary.run(|| unsafe {
        let result = crate::exception::catch_js_throw(|| {
            let ctx = context::get(component.get_nanbox_f64());
            if (*ctx).host.is_null() || !(*ctx).alive {
                fail("Component has been disposed");
            }
            imports::drain(owner.get_nanbox_f64(), component.get_nanbox_f64());
            if native(owner.get_nanbox_f64()).failed {
                let mut buffer = Buffer::default();
                (host::ops().perry_component_host_dispose)(
                    (*ctx).host,
                    cleanup.token(),
                    &mut buffer,
                );
                drop(take(buffer));
                status = RELEASED_ERROR;
                return undefined();
            }
            let mut buffer = Buffer::default();
            status = if !native(owner.get_nanbox_f64()).started {
                native(owner.get_nanbox_f64()).started = true;
                let op = native(owner.get_nanbox_f64());
                if op.http_request.is_null() {
                    (host::ops().perry_component_host_call_async_start)(
                        (*ctx).host,
                        op.interface.as_ptr(),
                        op.interface.len(),
                        op.function.as_ptr(),
                        op.function.len(),
                        op.args.as_ptr(),
                        op.args.len(),
                        primary.token(),
                        cleanup.token(),
                        Some(wake),
                        crate::agent::current_agent(),
                        &mut buffer,
                    )
                } else {
                    (host::ops().perry_component_http_handle_async_start)(
                        (*ctx).host,
                        op.http_request,
                        op.interface.as_ptr(),
                        op.interface.len(),
                        primary.token(),
                        cleanup.token(),
                        Some(wake),
                        crate::agent::current_agent(),
                        &mut buffer,
                    )
                }
            } else {
                (host::ops().perry_component_host_call_async_poll)(
                    (*ctx).host,
                    primary.token(),
                    cleanup.token(),
                    Some(wake),
                    crate::agent::current_agent(),
                    &mut buffer,
                )
            };
            output = take(buffer);
            if primary.threw.get() {
                remember(owner.get_nanbox_f64(), primary.error.get_nanbox_f64());
            }
            if status == ASYNC_PENDING {
                return undefined();
            }
            if status != OK {
                if !native(owner.get_nanbox_f64()).failed {
                    fail(&String::from_utf8_lossy(&output));
                }
                return undefined();
            }
            parsed = serde_json::from_slice(&output)
                .unwrap_or_else(|_| fail("Invalid async component result"));
            if parsed["returned"] == true {
                let op = native(owner.get_nanbox_f64());
                let signature =
                    context::signature(&(*ctx).world, "exports", &op.interface, &op.function);
                result_value.set_nanbox_f64(value::decode(
                    &parsed["value"],
                    &signature["result"],
                    &(*ctx).world,
                    component.get_nanbox_f64(),
                ));
            }
            undefined()
        });
        // Finalizers that ran while Wasm was suspended queued native
        // identities. Drain them after the Store is available again,
        // under the same captured context and primary-error precedence.
        if status != ASYNC_PENDING && status != RELEASED_ERROR {
            cleanup::finish_call(component.get_nanbox_f64(), result)
        } else {
            finish(result)
        }
    });
    if let Err(error) = outcome {
        remember(owner.get_nanbox_f64(), error);
    }
    if native(owner.get_nanbox_f64()).failed
        && status != HTTP_FAILURE
        && (status == ASYNC_PENDING || outcome.is_err())
        && native(owner.get_nanbox_f64()).started
        && unsafe { (*context::get(component.get_nanbox_f64())).alive }
    {
        let _ = cleanup.run_cleanup(|| {
            let mut buffer = Buffer::default();
            let ctx = context::get(component.get_nanbox_f64());
            unsafe {
                (host::ops().perry_component_host_dispose)(
                    (*ctx).host,
                    cleanup.token(),
                    &mut buffer,
                );
                drop(take(buffer));
            }
            undefined()
        });
        status = RELEASED_ERROR;
    }
    if status == RELEASED_ERROR {
        let disposed = cleanup.run_cleanup(|| {
            context::finish_disposal(component.get_nanbox_f64(), status);
            cleanup.rethrow();
            undefined()
        });
        if let Err(error) = disposed {
            remember(owner.get_nanbox_f64(), error);
        }
    }
    drop((output, parsed));
    if status == ASYNC_PENDING && outcome.is_ok() {
        return false;
    }
    let failed = native(owner.get_nanbox_f64()).failed;
    let primary_error = scope.root_nanbox_f64(payload::state_get(
        owner.get_nanbox_f64(),
        &OPERATION,
        b"error",
    ));
    unsafe { (*context::get(component.get_nanbox_f64())).async_active = false };
    finish_http(owner.get_nanbox_f64());
    payload::close(owner.get_nanbox_f64(), &OPERATION);
    let settled = Cell::new(false);
    let settled_result = unsafe {
        crate::async_hooks::try_run_promise_scope(
            JSValue::from_bits(promise.get_nanbox_f64().to_bits())
                .as_pointer::<crate::promise::Promise>()
                .cast_mut(),
            || {
                settled.set(true);
                let ptr = JSValue::from_bits(promise.get_nanbox_f64().to_bits())
                    .as_pointer::<crate::promise::Promise>()
                    .cast_mut();
                if failed {
                    crate::promise::js_promise_reject(ptr, primary_error.get_nanbox_f64());
                } else {
                    crate::promise::js_promise_resolve(ptr, result_value.get_nanbox_f64());
                }
                undefined()
            },
        )
    };
    if let Err(error) = settled_result {
        if settled.get() {
            crate::exception::js_throw(error);
        }
        crate::promise::js_promise_reject(
            JSValue::from_bits(promise.get_nanbox_f64().to_bits())
                .as_pointer::<crate::promise::Promise>()
                .cast_mut(),
            if failed {
                primary_error.get_nanbox_f64()
            } else {
                error
            },
        );
    }
    unsafe { wake(crate::agent::current_agent()) };
    true
}
fn finish_http(owner: f64) {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let http = payload::state_get(owner.get_nanbox_f64(), &OPERATION, b"http-owner");
    if JSValue::from_bits(http.to_bits()).is_pointer() {
        super::http::finish_delegation(http);
    }
}
extern "C" fn pump() -> i32 {
    if PUMPING.with(|v| v.replace(true)) {
        return 0;
    }
    let scope = RuntimeHandleScope::new();
    let first = scope.root_nanbox_f64(undefined());
    let mut threw = false;
    let mut completed = 0i32;
    let pending = PENDING.with(|q| std::mem::take(&mut *q.borrow_mut()));
    for link in pending {
        let result = crate::exception::catch_js_throw(|| {
            unsafe { payload::link_event_owner(link) }.map_or(true, step)
        });
        match result {
            Ok(false) => PENDING.with(|q| q.borrow_mut().push(link)),
            result => {
                if let Err(error) = result {
                    if !threw {
                        first.set_nanbox_f64(error);
                        threw = true;
                    }
                    if let Some(owner) = unsafe { payload::link_event_owner(link) } {
                        let component = payload::state_get(owner, &OPERATION, b"component");
                        unsafe { (*context::get(component)).async_active = false };
                        finish_http(owner);
                        payload::close(owner, &OPERATION);
                    }
                }
                unsafe { payload::link_unref(link) };
                completed = completed.saturating_add(1);
            }
        }
    }
    PUMPING.with(|v| v.set(false));
    if threw {
        crate::exception::js_throw(first.get_nanbox_f64());
    }
    completed
}
