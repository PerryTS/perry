//! Readers of an Array whose elements are not all plain slots of its store.
//!
//! `length` is the logical length. A sparse Array (`a[5_000_000] = x`,
//! `a.length = 5_000_000`, `new Array(5_000_000)`) keeps a short store under
//! it (`length > capacity`) and its far elements in the named layout, so a
//! loop that reads `length` raw slots walks off the store. Every method that
//! copies or searches by raw slots asks `array_iteration_is_exotic` first
//! (true for `length > capacity`) and takes one of the readers here instead.
use super::*;

/// The receiver's elements `[0, length)` in a fresh dense plain Array.
///
/// `keep_holes == false` is `Get(O, k)` for every `k` (`toReversed`,
/// `toSorted`, `toSpliced`, `with`): an absent index reads `undefined`.
/// `keep_holes == true` is `HasProperty(O, k)` then `Get(O, k)` (`concat`,
/// `splice`): an absent index stays a hole.
///
/// The result has `length <= capacity`, so its slots may be read raw. A
/// length the allocator cannot back throws `RangeError: Invalid array length`
/// before anything is read.
///
/// # Safety
/// `arr` is a live, forwarding-resolved plain Array (`clean_arr_ptr`'s
/// result). Getters can run and collect: the receiver and the result are
/// rooted, and the returned pointer is the result's address after the last
/// collection point.
#[cold]
#[inline(never)]
pub(super) unsafe fn exotic_snapshot(
    arr: *const ArrayHeader,
    keep_holes: bool,
) -> *mut ArrayHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let source = scope.root_raw_mut_ptr(arr as *mut ArrayHeader);
    let len = (*arr).length;
    let out = scope.root_raw_mut_ptr(js_array_alloc(len));
    {
        let out = out.get_raw_mut_ptr::<ArrayHeader>();
        if !keep_holes {
            // The fresh store is all holes; an absent index reads `undefined`.
            let slots = array_elements_ptr(out as *const ArrayHeader) as *mut u64;
            for i in 0..len as usize {
                // GC_STORE_AUDIT(POINTER_FREE): the immediate undefined tag into an unpublished array.
                std::ptr::write(slots.add(i), crate::value::TAG_UNDEFINED);
            }
        }
        (*out).length = len;
        rebuild_array_layout(out);
    }
    let read = |index: u32| -> Option<f64> {
        source.with_mut_ptr::<ArrayHeader, _>(|arr| {
            if crate::array::array_spec_has_index(arr, index) {
                Some(crate::array::array_spec_get(arr, index))
            } else {
                None
            }
        })
    };
    let store = |index: u32, value: f64| {
        // Published at once: the next getter can collect or throw.
        note_array_slot(
            out.get_raw_mut_ptr::<ArrayHeader>(),
            index as usize,
            value.to_bits(),
        );
    };
    // Nothing can be inherited through the default chain while no index
    // lives on a prototype: past the store only the named layout's own
    // index keys exist, and they are visited instead of every index.
    let (dense, inherit) = source.with_mut_ptr::<ArrayHeader, _>(|arr| {
        let inherit = array_object_flags_resolved(arr) & crate::gc::GC_ARRAY_CUSTOM_PROTO != 0
            || super::indexing::array_prototype_has_index_flag()
            || super::indexing::object_prototype_has_index_flag()
            || super::indexing_support::array_index_fast_path_invalid_for(0)
            || crate::object::prototype_chain::array_static_proto_recorded();
        ((*arr).length.min((*arr).capacity).min(len), inherit)
    });
    let walked = if inherit { len } else { dense };
    for index in 0..walked {
        if let Some(value) = read(index) {
            store(index, value);
        }
    }
    if walked < len {
        let mut far: Vec<u32> = source.with_mut_ptr::<ArrayHeader, _>(|arr| {
            super::named_props::array_named_property_names(arr, false)
                .into_iter()
                .filter_map(|name| crate::object::canonical_array_index(&name))
                .filter(|&index| index >= walked && index < len)
                .collect()
        });
        far.sort_unstable();
        far.dedup();
        for index in far {
            if let Some(value) = read(index) {
                store(index, value);
            }
        }
    }
    out.get_raw_mut_ptr::<ArrayHeader>()
}
