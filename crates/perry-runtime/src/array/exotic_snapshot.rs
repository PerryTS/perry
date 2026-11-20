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
        let far =
            source.with_mut_ptr::<ArrayHeader, _>(|arr| array_own_indices_in(arr, walked, len));
        for index in far {
            if let Some(value) = read(index) {
                store(index, value);
            }
        }
    }
    out.get_raw_mut_ptr::<ArrayHeader>()
}

/// The own index keys of `arr`'s named layout in `[from, to)`, ascending and
/// once each: where a sparse Array keeps the elements past its store.
///
/// # Safety
/// `arr` is a live, forwarding-resolved plain Array.
unsafe fn array_own_indices_in(arr: *const ArrayHeader, from: u32, to: u32) -> Vec<u32> {
    let mut far: Vec<u32> = super::named_props::array_named_property_names(arr, false)
        .into_iter()
        .filter_map(|name| crate::object::canonical_array_index(&name))
        .filter(|&index| index >= from && index < to)
        .collect();
    far.sort_unstable();
    far.dedup();
    far
}

/// Visit the own elements of a plain Array in index order, reading no slot
/// past its store: the present slots of `[0, min(length, capacity))`, then the
/// own index keys a sparse Array keeps in its named layout. A hole or an
/// absent index is not visited. `visit` returns `false` to stop.
///
/// For readers that show or inspect an Array (`util.inspect`, `console.log`,
/// `util.format`): it is no `[[Get]]` (an index on a prototype is not
/// visited), which is what node prints. Every bound is re-read after each
/// visit, so a visit that shortens the Array (an inspect hook) cannot make the
/// walk read a slot it gave up.
///
/// # Safety
/// `arr` is a live, forwarding-resolved plain Array (`clean_arr_ptr`'s
/// result) that does not move while the walk runs: a visit that can collect
/// must root it and pass its address back through a fresh call.
pub(crate) unsafe fn array_for_each_own_element(
    arr: *const ArrayHeader,
    mut visit: impl FnMut(u32, f64) -> bool,
) {
    let len = (*arr).length;
    let mut index = 0u32;
    while index < (*arr).length.min((*arr).capacity) {
        let bits = *(array_elements_ptr(arr) as *const u64).add(index as usize);
        if bits != crate::value::TAG_HOLE && !visit(index, f64::from_bits(bits)) {
            return;
        }
        index += 1;
    }
    if index >= len {
        return;
    }
    for far in array_own_indices_in(arr, index, len) {
        if far >= (*arr).length {
            return;
        }
        let Some(value) =
            super::named_props::array_named_property_get_by_name(arr, &far.to_string())
        else {
            continue;
        };
        if value.to_bits() != crate::value::TAG_HOLE && !visit(far, value) {
            return;
        }
    }
}
