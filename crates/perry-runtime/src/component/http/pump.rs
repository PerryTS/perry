use super::*;
use crate::native_payload::OwnerLink;
use perry_abi::component_host::{HTTP_PENDING, HTTP_READY, OK, RELEASED_ERROR};
use std::cell::{Cell, RefCell};

#[cfg(test)]
mod tests;

thread_local! {
    // Stable native cells, never JS addresses. Their pinned owner back-edges
    // are traced and rewritten by the ordinary payload descriptor.
    static PENDING: RefCell<Vec<OwnerLink>> = const { RefCell::new(Vec::new()) };
    static PUMPING: Cell<bool> = const { Cell::new(false) };
}

pub(super) fn notify(agent: crate::agent::AgentId) {
    #[cfg(not(target_arch = "wasm32"))]
    crate::event_pump::notify_agent_work(agent);
    #[cfg(target_arch = "wasm32")]
    let _ = agent;
}
unsafe extern "C" fn wake(agent: u64) {
    notify(agent);
}

fn retire(agent: crate::agent::AgentId) {
    // The arena finalizes even pinned cells during teardown. In particular,
    // never unref these links from a TLS destructor after arena unmapping.
    if agent == crate::agent::current_agent() {
        PENDING.with(|queue| queue.borrow_mut().clear());
    }
}

pub(super) fn enqueue(owner: f64) {
    let link = payload::owner_link(owner, &OPERATION).expect("live HTTP operation");
    crate::agent::register_retire_hook(retire);
    crate::stdlib_pump::js_register_agent_aux_pump(pump);
    crate::stdlib_pump::js_register_aux_has_active(has_active);
    unsafe { payload::link_ref(link) };
    PENDING.with(|queue| queue.borrow_mut().push(link));
    notify(crate::agent::current_agent());
}

extern "C" fn has_active() -> i32 {
    i32::from(PUMPING.with(Cell::get) || PENDING.with(|queue| !queue.borrow().is_empty()))
}

fn operation<'a>(owner: f64) -> &'a mut Operation {
    // Callers use this borrow only to copy native fields, never across JS.
    unsafe { payload::payload_mut::<Operation>(owner, &OPERATION) }
        .expect("queued HTTP operation is open")
}

fn remember(owner: f64, error: f64) {
    if operation(owner).failed {
        return;
    }
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    payload::state_set(owner.get_nanbox_f64(), &OPERATION, b"error", error);
    operation(owner.get_nanbox_f64()).failed = true;
}

/// Keep separate import and retirement frames: cleanup must never replace a
/// handler exception. All Rust-owned diagnostics live outside JS throw frames.
fn invoke(owner: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let component = scope.root_nanbox_f64(payload::state_get(
        owner.get_nanbox_f64(),
        &OPERATION,
        b"component",
    ));
    let primary = callback::Frame::new(&scope, component.get_nanbox_f64());
    let retirement = callback::Frame::new(&scope, component.get_nanbox_f64());
    let mut output = Vec::new();
    let mut status = OK;
    let result = crate::exception::catch_js_throw(|| {
        let context = context::get(component.get_nanbox_f64());
        if unsafe { (*context).host.is_null() || !(*context).alive } {
            fail("Component is not initialized or has been disposed");
        }
        let (pointer, interface, len) = {
            let native = operation(owner.get_nanbox_f64());
            (
                native.handle.pointer,
                native.interface.as_ptr(),
                native.interface.len(),
            )
        };
        let mut buffer = Buffer::default();
        status = unsafe {
            (host::ops().perry_component_http_handle)(
                (*context).host,
                pointer,
                interface,
                len,
                primary.token(),
                retirement.token(),
                &mut buffer,
            )
        };
        output = unsafe { take(buffer) };
        primary.rethrow();
        if status != OK {
            fail(&String::from_utf8_lossy(&output));
        }
        undefined()
    });
    let result_value = scope.root_nanbox_f64(*result.as_ref().unwrap_or_else(|e| e));
    let disposed = crate::exception::catch_js_throw(|| {
        if status == RELEASED_ERROR {
            context::finish_disposal(component.get_nanbox_f64(), status);
        }
        retirement.rethrow();
        undefined()
    });
    let result = if result.is_err() {
        Err(result_value.get_nanbox_f64())
    } else {
        disposed.map(|_| result_value.get_nanbox_f64())
    };
    drop(output);
    cleanup::finish_call(component.get_nanbox_f64(), result)
}

/// Returns true only when the operation has settled. Polling itself does no JS.
fn step(owner: f64) -> bool {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let promise = scope.root_nanbox_f64(payload::state_get(
        owner.get_nanbox_f64(),
        &OPERATION,
        b"promise",
    ));
    let mut output = Vec::new();
    let mut interface = String::new();
    let result = crate::exception::catch_js_throw(|| {
        if !operation(owner.get_nanbox_f64()).invoked {
            let component = payload::state_get(owner.get_nanbox_f64(), &OPERATION, b"component");
            let context = context::get(component);
            if unsafe { (*context).host.is_null() || !(*context).alive } {
                fail("Component is not initialized or has been disposed");
            }
        }
        let mut buffer = Buffer::default();
        let status = unsafe {
            (host::ops().perry_component_http_poll)(
                operation(owner.get_nanbox_f64()).handle.pointer,
                Some(wake),
                crate::agent::current_agent(),
                &mut buffer,
            )
        };
        output = unsafe { take(buffer) };
        match status {
            HTTP_PENDING => false,
            HTTP_READY if !operation(owner.get_nanbox_f64()).invoked => {
                operation(owner.get_nanbox_f64()).invoked = true;
                if operation(owner.get_nanbox_f64()).interface == "wasi:http/handler@0.3.0" {
                    interface = operation(owner.get_nanbox_f64()).interface.clone();
                    let pointer = operation(owner.get_nanbox_f64()).handle.pointer;
                    let component =
                        payload::state_get(owner.get_nanbox_f64(), &OPERATION, b"component");
                    if super::super::persistent::enabled(component) {
                        super::super::persistent::http::submit(
                            component,
                            &interface,
                            owner.get_nanbox_f64(),
                            pointer,
                            promise.get_nanbox_f64(),
                        );
                    } else {
                        super::super::asynchronous::submit_http(
                            component,
                            &interface,
                            owner.get_nanbox_f64(),
                            pointer,
                            promise.get_nanbox_f64(),
                        );
                    }
                    operation(owner.get_nanbox_f64()).delegated = true;
                    return true;
                }
                let result = unsafe {
                    crate::async_hooks::try_run_promise_scope(
                        JSValue::from_bits(promise.get_nanbox_f64().to_bits())
                            .as_pointer::<crate::promise::Promise>()
                            .cast_mut(),
                        || invoke(owner.get_nanbox_f64()),
                    )
                };
                if let Err(error) = result {
                    remember(owner.get_nanbox_f64(), error);
                }
                // Poll completion on the next pass, after Store and callback
                // frames have unwound. A reentrant pump cannot enter them.
                notify(crate::agent::current_agent());
                false
            }
            OK => true,
            _ => {
                if !operation(owner.get_nanbox_f64()).failed {
                    if status == HTTP_READY {
                        fail("Component HTTP handler did not consume the request");
                    }
                    fail(&String::from_utf8_lossy(&output));
                }
                true
            }
        }
    });
    drop((output, interface));
    // The async call driver now owns the public Promise and a traced edge to
    // this cell. Remove only our queue pin; it will close the native accept.
    if operation(owner.get_nanbox_f64()).delegated {
        return true;
    }
    let done = match result {
        Ok(done) => done,
        Err(error) => {
            remember(owner.get_nanbox_f64(), error);
            true
        }
    };
    if !done {
        return false;
    }
    let failed = operation(owner.get_nanbox_f64()).failed;
    let error = scope.root_nanbox_f64(payload::state_get(
        owner.get_nanbox_f64(),
        &OPERATION,
        b"error",
    ));
    // Release native resources before observable Promise hooks run.
    payload::close(owner.get_nanbox_f64(), &OPERATION);
    let settled = Cell::new(false);
    let outcome = unsafe {
        crate::async_hooks::try_run_promise_scope(
            JSValue::from_bits(promise.get_nanbox_f64().to_bits())
                .as_pointer::<crate::promise::Promise>()
                .cast_mut(),
            || {
                settled.set(true);
                let pointer = JSValue::from_bits(promise.get_nanbox_f64().to_bits())
                    .as_pointer::<crate::promise::Promise>()
                    .cast_mut();
                if failed {
                    crate::promise::js_promise_reject(pointer, error.get_nanbox_f64());
                } else {
                    crate::promise::js_promise_resolve(pointer, undefined());
                }
                undefined()
            },
        )
    };
    if let Err(hook_error) = outcome {
        // A throwing before hook must not strand a pending Promise. A throw
        // after settlement propagates through the event pump after unpinning.
        if settled.get() {
            crate::exception::js_throw(hook_error);
        }
        let reason = if failed {
            error.get_nanbox_f64()
        } else {
            hook_error
        };
        crate::promise::js_promise_reject(
            JSValue::from_bits(promise.get_nanbox_f64().to_bits())
                .as_pointer::<crate::promise::Promise>()
                .cast_mut(),
            reason,
        );
    }
    true
}

extern "C" fn pump() -> i32 {
    if PUMPING.with(|busy| busy.replace(true)) {
        return 0;
    }
    let scope = RuntimeHandleScope::new();
    let first_error = scope.root_nanbox_f64(undefined());
    let mut threw = false;
    let mut completed = 0i32;
    // Detached items remain pinned while any callback collects the heap.
    let pending = PENDING.with(|queue| std::mem::take(&mut *queue.borrow_mut()));
    for link in pending {
        let result = crate::exception::catch_js_throw(|| {
            unsafe { payload::link_event_owner(link) }.map_or(true, step)
        });
        match result {
            Ok(false) => PENDING.with(|queue| queue.borrow_mut().push(link)),
            result => {
                if let Err(error) = result {
                    if !threw {
                        first_error.set_nanbox_f64(error);
                        threw = true;
                    }
                    if let Some(owner) = unsafe { payload::link_event_owner(link) } {
                        payload::close(owner, &OPERATION);
                    }
                }
                unsafe { payload::link_unref(link) };
                completed = completed.saturating_add(1);
            }
        }
    }
    PUMPING.with(|busy| busy.set(false));
    if threw {
        crate::exception::js_throw(first_error.get_nanbox_f64());
    }
    completed
}
