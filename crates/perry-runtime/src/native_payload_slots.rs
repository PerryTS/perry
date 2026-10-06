//! Traced callback-array access on the stable payload cell.

use super::*;

/// Only callback families pay for this traced extension. The legacy/N header
/// and its layout remain unchanged; CALLBACK_STORAGE proves the allocation.
#[repr(C)]
pub(crate) struct NativeCallbackCell {
    pub header: NativeHandleHeader,
    pub callbacks: u64,
    pub catch: *mut crate::exception::NativeCatch,
}

#[inline]
pub(crate) unsafe fn callback_slot_address(cell: *mut NativeHandleHeader) -> Option<*mut u64> {
    ((*cell).flags & CALLBACK_STORAGE != 0)
        .then(|| &raw mut (*(cell as *mut NativeCallbackCell)).callbacks)
}

/// The callback registered at `index` in the owner's callbacks array
/// (`undefined` when absent). Never allocates.
pub fn callback_at(owner: f64, family: &NativePayloadFamily, index: u32) -> f64 {
    let Ok(cell) = payload_cell(owner, family.class_id) else {
        return undefined();
    };
    unsafe {
        if callback_slot_address(cell).is_some() {
            callback_slot(cell, index)
        } else {
            let obj = instance_of(owner, family.class_id).unwrap();
            raw_js_state(obj).map_or_else(undefined, |state| {
                let array = raw_field_memo(state, b"callbacks", &CALLBACKS_MEMO);
                if !is_callback_array(crate::JSValue::from_bits(array.to_bits())) {
                    return undefined();
                }
                callback_array_slot(array.to_bits(), index)
            })
        }
    }
}

/// Read a callback through stable userdata, after its last allocating conversion.
/// Never allocates; the slot is rewritten by moving GC. Whether the owner
/// may still be called is [`call_from_link`]'s check, made at the call.
///
/// # Safety
/// `link` passed [`link_owner`] on this thread in the calling trampoline.
/// The caller roots pointer arguments until the JS call.
#[inline]
pub unsafe fn callback_from_link(link: OwnerLink, index: u32) -> f64 {
    callback_slot(link.0 as *mut NativeHandleHeader, index)
}

/// Call the callback at `index` of the owner `link` names: the whole JS side
/// of an S trampoline that needs nothing else from the owner. One check that
/// the owner may run JS (open, on this thread, nothing pending) answers
/// `Err` with no JS otherwise; then the slot is read and called under the
/// native call's catch, as [`call_from_link`] calls. The caller keeps every
/// pointer in `args` rooted, or converted them after its last allocation.
///
/// # Safety
/// `link` names a live cell, owned by its C resource.
#[inline]
pub unsafe fn call_callback(link: OwnerLink, index: u32, args: &[f64]) -> Result<f64, ()> {
    link_owner(link).ok_or(())?;
    let cell = link.0 as *mut NativeHandleHeader;
    if (*cell).flags & PENDING != 0 && !pending_sabotage() {
        return Err(());
    }
    let callee = callback_slot(cell, index);
    if (*cell).flags & PENDING_SLOT == 0 {
        return first_call_from_link(link, callee, args);
    }
    call_open_link(cell, callee, args)
}

#[inline(always)]
unsafe fn callback_slot(cell: *mut NativeHandleHeader, index: u32) -> f64 {
    let Some(slot) = callback_slot_address(cell) else {
        return undefined();
    };
    callback_array_slot(*slot, index)
}

#[inline(always)]
unsafe fn callback_array_slot(bits: u64, index: u32) -> f64 {
    let array = crate::JSValue::from_bits(bits);
    if !array.is_pointer() {
        return undefined();
    }
    let arr = array.as_pointer::<crate::array::ArrayHeader>();
    // The traced slot proves a real array, so ordinary reads need one
    // header bit; growth may leave a forwarding stub before the next GC
    // rewrite, and an index past the dense storage is a sparse own key.
    let header = crate::gc::header_from_trusted_user_ptr(arr.cast());
    #[cfg(test)]
    let test_path =
        callback_sabotage("callback_forwarding") || callback_sabotage("callback_data_read");
    #[cfg(not(test))]
    let test_path = false;
    if test_path
        || (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || index >= (*arr).capacity
    {
        return callback_array_slot_slow(arr, index);
    }
    dense_callback(arr, index)
}

#[inline(always)]
unsafe fn dense_callback(arr: *const crate::array::ArrayHeader, index: u32) -> f64 {
    if index >= (*arr).length {
        return undefined();
    }
    let bits = *crate::array::array_elements_ptr(arr).add(index as usize);
    if bits == crate::value::TAG_HOLE {
        undefined()
    } else {
        f64::from_bits(bits)
    }
}

#[cold]
#[inline(never)]
unsafe fn callback_array_slot_slow(mut arr: *const crate::array::ArrayHeader, index: u32) -> f64 {
    let header = crate::gc::header_from_trusted_user_ptr(arr.cast());
    let forwarded = (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0;
    #[cfg(test)]
    let forwarded = forwarded && !callback_sabotage("callback_forwarding");
    if forwarded {
        arr = crate::array::clean_arr_ptr(arr);
        if arr.is_null() {
            return undefined();
        }
    }
    #[cfg(test)]
    if callback_sabotage("callback_data_read") {
        return f64::from_bits(crate::array::js_array_get(arr, index).bits());
    }
    if index >= (*arr).length {
        return undefined();
    }
    if index >= (*arr).capacity {
        return sparse_callback_slot(arr, index);
    }
    dense_callback(arr, index)
}

pub(super) unsafe fn store_callbacks(cell: *mut NativeHandleHeader, value: f64) {
    // GC_STORE_AUDIT(BARRIERED): exact malloc-parent edge, just like owner.
    #[cfg(test)]
    if callback_sabotage("callback_sync") {
        return;
    }
    let value = crate::JSValue::from_bits(value.to_bits());
    let bits = if is_callback_array(value) {
        value.bits()
    } else {
        crate::value::TAG_UNDEFINED
    };
    let Some(slot) = callback_slot_address(cell) else {
        return;
    };
    *slot = bits;
    #[cfg(test)]
    if callback_sabotage("callback_barrier") {
        return;
    }
    crate::gc::runtime_write_barrier_external_slot(cell as usize, slot as usize, bits);
}

pub(super) fn sync_callbacks(owner: f64, family: &NativePayloadFamily, key: &[u8], value: f64) {
    if key == b"callbacks" {
        if let Ok(cell) = payload_cell(owner, family.class_id) {
            unsafe { store_callbacks(cell, value) };
        }
    }
}

#[inline]
unsafe fn is_callback_array(value: crate::JSValue) -> bool {
    value.is_pointer()
        && crate::value::addr_class::try_read_gc_header(value.as_pointer::<u8>() as usize)
            .is_some_and(|h| h.obj_type == crate::gc::GC_TYPE_ARRAY)
}

pub(super) unsafe fn catch_from_cell(
    cell: *mut NativeHandleHeader,
) -> *mut crate::exception::NativeCatch {
    if (*cell).flags & CALLBACK_STORAGE == 0 {
        return std::ptr::null_mut();
    }
    (*(cell as *mut NativeCallbackCell)).catch
}

#[cold]
unsafe fn sparse_callback_slot(arr: *const crate::array::ArrayHeader, mut index: u32) -> f64 {
    let mut digits = [0u8; 10];
    let mut start = digits.len();
    loop {
        start -= 1;
        digits[start] = b'0' + (index % 10) as u8;
        index /= 10;
        if index == 0 {
            break;
        }
    }
    #[cfg(test)]
    if callback_sabotage("callback_sparse_key") {
        digits[start] = b'x';
    }
    // Every byte above is ASCII. No heap or GC allocation.
    let key = std::str::from_utf8_unchecked(&digits[start..]);
    crate::array::array_named_property_get_by_name(arr, key).unwrap_or_else(undefined)
}
