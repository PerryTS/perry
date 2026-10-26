//! Persistent component ownership on the creating agent. Native metadata holds
//! IDs and row indices; ordinary managed arrays own every JavaScript edge.
mod callbacks;
pub(super) mod collections;
pub(super) mod http;
pub(super) mod imports;
pub(super) mod operations;
mod poll;
mod records;
pub(super) mod submit;
#[cfg(test)]
mod tests;

use super::*;
use crate::native_payload::{self as payload, NativePayloadFamily};
use perry_abi::component_host::{ERROR, OK, RELEASED_ERROR};
use records::{Origin, ReplyKind};
use std::{
    collections::{BTreeMap, VecDeque},
    ffi::c_void,
};

static PUMP: NativePayloadFamily = NativePayloadFamily {
    class_id: crate::native_class_ids::COMPONENT_DRIVER_PUMP,
    links_owner: true,
    name: "ComponentDriverPump",
    constructor_export: None,
    constructor_length: 0,
    install_prototype: |_| {},
};
struct DriverPump;
#[derive(Default)]
pub(super) struct State {
    origins: BTreeMap<u64, Origin>,
    ready: BTreeMap<u64, Value>,
    ready_order: VecDeque<u64>,
    free: Vec<u32>,
    slots: u32,
    admission: u64,
    queued: bool,
    polling: bool,
    cycling: bool,
    retired: bool,
    abort: bool,
    active: bool,
    root_failed: bool,
}
fn state(component: f64) -> *mut State {
    unsafe {
        (*context::get(component))
            .persistent
            .as_mut()
            .expect("persistent component")
    }
}
pub(super) fn enabled(component: f64) -> bool {
    unsafe { (*context::get(component)).persistent.is_some() }
}
fn owner(component: f64) -> f64 {
    unsafe {
        crate::object::owned_state_bag::load(
            object_pointer(component).unwrap() as usize,
            crate::object::owned_state_bag::COMPONENT_DRIVER_OWNER,
        )
    }
}
fn initial_row(driver_owner: f64) -> f64 {
    payload::state_get(driver_owner, &PUMP, b"initial")
}
fn origin_row(component: f64, origin: u64) -> f64 {
    let driver_owner = owner(component);
    if origin == 0 {
        return initial_row(driver_owner);
    }
    let slot = unsafe {
        (*state(component))
            .origins
            .get(&origin)
            .map(|meta| meta.slot)
    }
    .unwrap_or_else(|| fail("Unknown persistent component invocation"));
    records::row(records::table(driver_owner), slot)
}
fn ids(component: f64, origin: u64) -> crate::async_hooks::AsyncResourceIds {
    if origin == 0 {
        return crate::async_hooks::AsyncResourceIds {
            async_id: 0,
            trigger_async_id: 0,
        };
    }
    unsafe {
        (*state(component))
            .origins
            .get(&origin)
            .expect("known invocation")
            .ids
    }
}
fn remember(component: f64, origin: u64, error: f64) {
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(component);
    let error = scope.root_nanbox_f64(error);
    let first = unsafe {
        let data = &mut *state(component.get_nanbox_f64());
        if origin == 0 {
            let first = !data.root_failed;
            data.root_failed = true;
            first
        } else if let Some(meta) = data.origins.get_mut(&origin) {
            let first = !meta.failed;
            meta.failed = true;
            first
        } else {
            false
        }
    };
    if first {
        records::set(
            origin_row(component.get_nanbox_f64(), origin),
            records::ERROR,
            error.get_nanbox_f64(),
        );
    }
}
fn prepare(component: f64) {
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(component);
    let driver_owner = scope.root_nanbox_f64(payload::alloc_closed(&PUMP, &[]));
    payload::state_set(
        driver_owner.get_nanbox_f64(),
        &PUMP,
        b"component",
        component.get_nanbox_f64(),
    );
    for key in [b"rows".as_slice(), b"imports".as_slice()] {
        let list = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::array::js_array_alloc(0) as i64,
        ));
        payload::state_set(
            driver_owner.get_nanbox_f64(),
            &PUMP,
            key,
            list.get_nanbox_f64(),
        );
    }
    let captured = scope.root_nanbox_f64(callback::saved::current());
    let initial = scope.root_nanbox_f64(records::create(
        undefined(),
        undefined(),
        captured.get_nanbox_f64(),
    ));
    payload::state_set(
        driver_owner.get_nanbox_f64(),
        &PUMP,
        b"initial",
        initial.get_nanbox_f64(),
    );
    payload::attach(driver_owner.get_nanbox_f64(), &PUMP, DriverPump, 0)
        .expect("new component pump");
    unsafe {
        crate::object::owned_state_bag::store(
            object_pointer(component.get_nanbox_f64()).unwrap() as usize,
            crate::object::owned_state_bag::COMPONENT_DRIVER_OWNER,
            driver_owner.get_nanbox_f64(),
        );
        (*context::get(component.get_nanbox_f64())).persistent = Some(State::default());
    }
}

pub(super) fn construct(
    component: f64,
    wasm: &[u8],
    contract: &[u8],
    options: &[u8],
) -> *mut c_void {
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(component);
    prepare(component.get_nanbox_f64());
    let batch = callbacks::Batch::new(&scope, component.get_nanbox_f64(), false);
    let mut bytes = Vec::new();
    let outcome = crate::exception::catch_js_throw(|| {
        let mut buffer = Buffer::default();
        let pointer = unsafe {
            (host::ops().perry_component_driver_new)(
                wasm.as_ptr(),
                wasm.len(),
                contract.as_ptr(),
                contract.len(),
                options.as_ptr(),
                options.len(),
                Some(callbacks::invoke),
                Some(callback::free),
                Some(callbacks::resource_event),
                batch.token(),
                &mut buffer,
            )
        };
        bytes = unsafe { take(buffer) };
        unsafe {
            (*context::get(component.get_nanbox_f64())).host = pointer;
        }
        batch.rethrow_callback_error();
        if pointer.is_null() {
            fail(&String::from_utf8_lossy(&bytes));
        }
        super::producer::install(component.get_nanbox_f64());
        poll::schedule(component.get_nanbox_f64());
        pointer
    });
    let failure = outcome.err().map(|error| scope.root_nanbox_f64(error));
    drop((batch, bytes));
    if let Some(error) = failure {
        crate::exception::js_throw(error.get_nanbox_f64());
    }
    outcome.unwrap()
}

pub(super) fn call(
    component: f64,
    interface: &str,
    function: &str,
    args: f64,
    asynchronous: bool,
) -> f64 {
    submit::call(component, interface, function, args, asynchronous)
}
pub(super) fn dispose(component: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(component);
    if unsafe { (*state(component.get_nanbox_f64())).retired } {
        return undefined();
    }
    if unsafe { (*state(component.get_nanbox_f64())).polling } {
        fail("Cannot recursively execute or dispose a component");
    }
    let batch = callbacks::Batch::new(&scope, component.get_nanbox_f64(), true);
    let error = scope.root_nanbox_f64(undefined());
    let mut bytes = Vec::new();
    let outcome = crate::exception::catch_js_throw(|| {
        let mut buffer = Buffer::default();
        unsafe {
            (*state(component.get_nanbox_f64())).polling = true;
        }
        let status = unsafe {
            (host::ops().perry_component_host_dispose)(
                (*context::get(component.get_nanbox_f64())).host,
                batch.token(),
                &mut buffer,
            )
        };
        unsafe {
            (*state(component.get_nanbox_f64())).polling = false;
        }
        bytes = unsafe { take(buffer) };
        if matches!(status, OK | RELEASED_ERROR) {
            unsafe {
                let data = &mut *state(component.get_nanbox_f64());
                data.retired = true;
                for meta in data.origins.values_mut() {
                    meta.retained = false;
                }
                (*context::get(component.get_nanbox_f64())).alive = false;
            }
            let rejected = crate::exception::catch_js_throw(|| {
                poll::reject_pending(component.get_nanbox_f64(), "Component has been disposed")
            });
            let disposed = batch.cleanup(|| {
                context::finish_disposal(component.get_nanbox_f64(), status);
                undefined()
            });
            // Native callbacks may use a retained invocation's origin, not
            // only origin zero. Its first exception precedes later local cleanup.
            batch.callback_error().map_or(Ok(()), Err)?;
            rejected?;
            disposed?;
        }
        batch.callback_error().map_or(Ok(()), Err)?;
        if status != OK {
            fail(&String::from_utf8_lossy(&bytes));
        }
        Ok::<(), f64>(())
    });
    let outcome = match outcome {
        Ok(inner) => inner,
        Err(error) => Err(error),
    };
    if let Err(thrown) = outcome {
        error.set_nanbox_f64(thrown);
    }
    drop((batch, bytes));
    if outcome.is_err() {
        crate::exception::js_throw(error.get_nanbox_f64());
    }
    undefined()
}
