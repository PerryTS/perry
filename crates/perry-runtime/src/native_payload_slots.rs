//! Traced callback-array access on the stable payload cell.

use super::*;

/// The callback registered at `index` in the owner's callbacks array
/// (`undefined` when absent). Never allocates.
pub fn callback_at(owner: f64, family: &NativePayloadFamily, index: u32) -> f64 {
    let Ok(cell) = payload_cell(owner, family.class_id) else {
        return undefined();
    };
    unsafe { callback_slot(cell, index) }
}

/// Read a callback through stable userdata, after its last allocating conversion.
/// Never allocates; the slot is rewritten by moving GC.
///
/// # Safety
/// As [`link_owner`]. The caller roots pointer arguments until the JS call.
#[inline]
pub unsafe fn callback_from_link(link: OwnerLink, index: u32) -> f64 {
    if link_owner(link).is_none() {
        return undefined();
    }
    callback_slot(link.0 as *mut NativeHandleHeader, index)
}

#[inline]
unsafe fn callback_slot(cell: *mut NativeHandleHeader, index: u32) -> f64 {
    let array = crate::JSValue::from_bits((*cell).callbacks);
    if !array.is_pointer() {
        return undefined();
    }
    let arr = array.as_pointer::<crate::array::ArrayHeader>();
    if index >= crate::array::js_array_length(arr) {
        return undefined();
    }
    f64::from_bits(crate::array::js_array_get(arr, index).bits())
}

pub(super) unsafe fn store_callbacks(cell: *mut NativeHandleHeader, value: f64) {
    // GC_STORE_AUDIT(BARRIERED): exact malloc-parent edge, just like owner.
    #[cfg(test)]
    if callback_sabotage("callback_sync") {
        return;
    }
    (*cell).callbacks = value.to_bits();
    #[cfg(test)]
    if callback_sabotage("callback_barrier") {
        return;
    }
    crate::gc::runtime_write_barrier_external_slot(
        cell as usize,
        &(*cell).callbacks as *const _ as usize,
        value.to_bits(),
    );
}

pub(super) fn sync_callbacks(owner: f64, family: &NativePayloadFamily, key: &[u8], value: f64) {
    if key == b"callbacks" {
        if let Ok(cell) = payload_cell(owner, family.class_id) {
            unsafe { store_callbacks(cell, value) };
        }
    }
}
