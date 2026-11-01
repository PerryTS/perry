//! Capacity growth shared by compiled appends and single-value push.
use super::*;

/// Shared continuation for source-level `a[a.length] = value`. The evaluated
/// key remains authoritative if the RHS changed length. Slow shapes use the
/// complete Set operation, including strictness and inherited setters.
/// `current` is the caller's binding value AFTER evaluating the RHS. Return
/// a healed head only if it still owns the evaluated receiver. Callers with
/// globals/captured bindings discard this result and retain forwarding.
#[inline(never)]
#[no_mangle]
pub unsafe extern "C" fn js_array_append_set(
    receiver: f64,
    index: f64,
    value: f64,
    strict: i32,
    current: f64,
) -> f64 {
    if let Some(arr) = admitted_append(receiver, index) {
        crate::string::js_string_addref_if_heap_string(value);
        let length = (*arr).length;
        let flags = array_object_flags_resolved(arr);
        let live = if length < (*arr).capacity {
            let pointer = crate::gc::layout_pointer_bearing_bits(value.to_bits());
            let layout =
                flags & (crate::gc::GC_LAYOUT_STATE_MASK | crate::gc::GC_LAYOUT_ALL_POINTERS);
            let kind_plain = flags
                & (crate::gc::GC_ARRAY_RAW_F64_LAYOUT
                    | crate::gc::GC_ARRAY_RAW_F64_HOLES
                    | crate::gc::GC_ARRAY_ELEMENT_SHAPE)
                == 0;
            let note_noop = kind_plain
                && (layout == 0
                    || (!pointer && layout == crate::gc::GC_LAYOUT_POINTER_FREE)
                    || (pointer && layout == 0xA000));
            if note_noop {
                let slot = array_elements_ptr(arr).add(length as usize);
                // GC_STORE_AUDIT(BARRIERED): a shared exact slot barrier below.
                std::ptr::write(slot, value.to_bits());
                crate::gc::runtime_write_barrier_resolved_slot(
                    arr as usize,
                    slot as usize,
                    value.to_bits(),
                );
            } else {
                // Kind conversion, homogeneous element proof and layout changes
                // all stay here, shared across sites, before length is published.
                store_array_slot_resolved(arr, length as usize, value, flags);
            }
            (*arr).length = length + 1;
            crate::array::ek_note_published(arr, length as usize);
            arr
        } else {
            let scope = crate::gc::RuntimeHandleScope::new();
            let receiver_root = scope.root_nanbox_f64(receiver);
            let current_root = scope.root_nanbox_f64(current);
            let live = grow(arr, length, value);
            let current = current_root.get_nanbox_f64();
            return if current.to_bits() == receiver_root.get_nanbox_f64().to_bits() {
                crate::value::js_nanbox_pointer(live as i64)
            } else {
                current
            };
        };
        return if current.to_bits() == receiver.to_bits() {
            crate::value::js_nanbox_pointer(live as i64)
        } else {
            current
        };
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let current = scope.root_nanbox_f64(current);
    let (_, current) = current
        .across_nanbox(|| crate::value::js_dyn_index_set_strict(receiver, index, value, strict));
    current
}

#[inline]
unsafe fn admitted_append(receiver: f64, index: f64) -> Option<*mut ArrayHeader> {
    let bits = receiver.to_bits();
    if bits & crate::value::TAG_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let raw = (bits & crate::value::POINTER_MASK) as usize;
    let header = crate::value::addr_class::try_read_gc_header(raw)?;
    if header.obj_type != crate::gc::GC_TYPE_ARRAY {
        return None;
    }
    let arr = if header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        clean_arr_ptr_mut(raw as *mut ArrayHeader)
    } else {
        raw as *mut ArrayHeader
    };
    if arr.is_null() {
        return None;
    }
    // The intrinsic prototype owns named methods and constructor, so its
    // descriptor/own-key shape also declines here, without an address lookup.
    const REJECT: u16 = crate::gc::OBJ_FLAG_FROZEN
        | crate::gc::OBJ_FLAG_SEALED
        | crate::gc::OBJ_FLAG_NO_EXTEND
        | crate::gc::GC_ARRAY_CUSTOM_PROTO;
    if array_object_flags_resolved(arr) & REJECT != 0
        || array_has_element_descriptors_resolved(arr)
        || PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED.load(std::sync::atomic::Ordering::Relaxed)
            & super::INDEX_INVALIDATED
            != 0
        || value_bits_to_number(index.to_bits()) != Some((*arr).length as f64)
        || (*arr).length > (*arr).capacity
    {
        return None;
    }
    Some(arr)
}

#[cold]
#[optimize(size)]
#[inline(never)]
pub(super) unsafe fn grow(arr: *mut ArrayHeader, length: u32, value: f64) -> *mut ArrayHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let _arr_handle = scope.root_raw_mut_ptr(arr);
    let value_handle = scope.root_nanbox_f64(value);

    let (arr, value) = value_handle.across_nanbox(|| js_array_grow(arr, length + 1));
    // SAFETY: `js_array_grow` returns the resolved live array head and no
    // safepoint intervenes before the flag read/store.
    let flags = array_object_flags_resolved(arr);
    // GC_STORE_AUDIT(BARRIERED): the resolved store performs the layout note
    // and write barrier as part of the slot write.
    store_array_slot_resolved(arr, length as usize, value, flags);
    (*arr).length = length + 1;
    crate::array::ek_note_published(arr, length as usize);
    arr
}
