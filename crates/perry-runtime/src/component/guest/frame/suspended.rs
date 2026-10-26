//! A deferred canonical import owns its existing traced frame through C.
//! Only slot indices live in native metadata; the parent owns the task lease.
use super::*;
use crate::async_context::{self as als, job_domain};

pub(super) struct SuspendedImport {
    snapshot: u32,
    promise: u32,
    settled: bool,
}

/// Publish a Promise and captured context before starting a canonical child.
/// The runtime hands the stable frame to C when its import callback returns.
/// # Safety
/// `pointer` is a heap import frame on its creating agent, retained by its C
/// owner. It is not a task frame and must not have begun another import.
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_import_begin(pointer: *mut c_void) -> u32 {
    frame(pointer).guard(|owner| {
        if owner.task.is_some() || owner.suspended.is_some() {
            fail("Expected a fresh component import frame");
        }
        if !job_domain::current().is_some_and(|token| token.is_current_live()) {
            fail("Async component import requires a live canonical task");
        }
        let scope = RuntimeHandleScope::new();
        let promise = scope.root_raw_mut_ptr(crate::promise::js_promise_new());
        let promise = owner.add(crate::value::js_nanbox_pointer(
            promise.get_raw_mut_ptr::<crate::promise::Promise>() as i64,
        ));
        let saved = als::alloc_snapshot_holder(als::capture_context());
        let snapshot = owner.add(crate::value::js_nanbox_pointer(saved as i64));
        owner.suspended = Some(SuspendedImport {
            snapshot,
            promise,
            settled: false,
        });
        promise
    })
}

unsafe fn settle(pointer: *mut c_void, value: u32, reject: bool) -> i32 {
    let owner = frame(pointer);
    owner.guard(|owner| {
        let state = owner
            .suspended
            .as_ref()
            .unwrap_or_else(|| fail("Expected a deferred component import"));
        if state.settled {
            fail("Component import already completed");
        }
        let promise_slot = state.promise;
        let snapshot_slot = state.snapshot;
        let scope = RuntimeHandleScope::new();
        let value = scope.root_nanbox_f64(owner.get(value));
        let promise = scope.root_nanbox_f64(owner.get(promise_slot));
        let snapshot = (*JSValue::from_bits(owner.get(snapshot_slot).to_bits())
            .as_pointer::<als::AsyncSnapshotHolder>())
        .snapshot
        .clone();
        if !snapshot
            .job_domain_token()
            .is_some_and(|token| token.is_current_live())
        {
            drop(snapshot);
            fail("Component import completed outside its originating task");
        }
        let previous = als::enter_context(&snapshot);
        drop(snapshot);
        als::push_context_guard(als::ContextGuardAction::RestoreSnapshot(previous));
        // Settlement may invoke JS hooks. A reentrant completion must not get
        // a second attempt, including when the first hook throws.
        owner.suspended.as_mut().unwrap().settled = true;
        let promise = JSValue::from_bits(promise.get_nanbox_f64().to_bits())
            .as_pointer::<crate::promise::Promise>()
            .cast_mut();
        if reject {
            crate::promise::js_promise_reject(promise, value.get_nanbox_f64());
        } else {
            crate::promise::assimilate::promise_resolve_assimilating(
                promise,
                value.get_nanbox_f64(),
            );
        }
        if let Some(action) = als::pop_context_guard() {
            als::apply_context_guard(action);
        }
    });
    if owner.failed {
        -1
    } else {
        0
    }
}

/// Settle once under the live originating task, restoring its captured ALS.
/// # Safety
/// `pointer` is an open deferred heap import frame on its creating agent;
/// `value` is a slot in that frame. C retains it until canonical child cleanup.
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_import_resolve(
    pointer: *mut c_void,
    value: u32,
) -> i32 {
    settle(pointer, value, false)
}

/// Reject once with the original rooted JS reason, under the originating task.
/// # Safety
/// The frame, agent and slot requirements are the same as import_resolve.
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_import_reject(
    pointer: *mut c_void,
    value: u32,
) -> i32 {
    settle(pointer, value, true)
}

#[cfg(test)]
mod tests;
