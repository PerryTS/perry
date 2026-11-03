//! Unobserved native operations may be collected with their public Promise.
//! Once a reaction is attached, the producer must retain that Promise so an
//! awaited temporary cannot lose its continuation. Ordinary Promises have no
//! link and take the no-allocation fast path.
use super::*;
const OPERATION: u32 = 3;
const ROW: u32 = 0;
const SLOT: u32 = 1;

/// The public Promise owns its temporary handle. The producer initially stores
/// only a WeakRef in `row[slot]`; observation replaces it with the Promise.
/// No JS callback or allocation runs in the observation hook.
pub(crate) fn register(promise: *mut Promise, row: f64, slot: u32, owner: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let promise = scope.root_raw_mut_ptr(promise);
    let row = scope.root_nanbox_f64(row);
    let owner = scope.root_nanbox_f64(owner);
    let _publication = crate::gc::GcSuppressScope::new();
    unsafe {
        owned_state::push_record(
            promise.get_raw_mut_ptr(),
            OPERATION,
            &[row.get_nanbox_f64(), slot as f64, owner.get_nanbox_f64()],
        );
        let pointer = promise.get_raw_mut_ptr::<Promise>();
        // An init hook can have attached a reaction before registration.
        if !(*pointer).on_fulfilled.is_null()
            || !(*pointer).on_rejected.is_null()
            || !(*pointer).next.is_null()
            || [
                owned_state::ALL_STATES,
                owned_state::LISTENERS,
                owned_state::OVERFLOW,
            ]
            .into_iter()
            .any(|slot| !owned_state::state_list(pointer, slot).is_null())
        {
            observe(pointer);
        }
    }
}

pub(crate) fn observe(promise: *mut Promise) {
    if promise.is_null() || unsafe { (*promise).state != PromiseState::Pending } {
        return;
    }
    unsafe {
        let link = owned_state::state_list(promise, OPERATION);
        if link.is_null() {
            return;
        }
        let row = crate::array::js_array_get_f64(link, ROW);
        let slot = crate::array::js_array_get_f64(link, SLOT) as u32;
        crate::array::js_array_set_f64(
            crate::value::js_nanbox_get_pointer(row) as *mut crate::array::ArrayHeader,
            slot,
            crate::value::js_nanbox_pointer(promise as i64),
        );
    }
}

pub(crate) fn clear(promise: *mut Promise) {
    unsafe {
        owned_state::take_list(promise, OPERATION);
    }
}
