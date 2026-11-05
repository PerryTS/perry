//! The element-kind word on [`ArrayHeader`] (EK design §3): the dense prefix
//! `ek_prefix` (P) and the value tag `ek_kind`.
//!
//! P is a lower bound. Every slot below P holds a plain data element: not
//! `TAG_HOLE`, and no element descriptor in the array's `named` layout. Also
//! `P <= min(length, capacity)`. Lowering P is always sound; every raise
//! below proves the slots it covers. An operation that can make a slot below
//! P a hole, give it a descriptor, move it, or shrink `length` below P calls
//! one of these helpers. The tag stays ANY (0) until the numeric consumers
//! move to it (phase B3).
//!
//! The descriptor clamp (§3.1): a row proves only that the slots it covers
//! are non-holes, never that a key in `[P, P')` has no descriptor. So P does
//! not rise while the array holds element descriptors.

use super::ArrayHeader;

/// Lower P to at most `bound`.
///
/// # Safety
/// `arr` is a live, forwarding-resolved array head.
#[inline(always)]
pub(crate) unsafe fn ek_lower_prefix(arr: *mut ArrayHeader, bound: u32) {
    if (*arr).ek_prefix > bound {
        (*arr).ek_prefix = bound;
    }
}

/// Install `proved` as the new prefix after a row that rewrote or moved
/// slots: the caller's formula proves `[0, proved)` non-hole for the
/// current contents. A rise over the old prefix is dropped while the array
/// holds element descriptors (the descriptor clamp); the result never
/// exceeds `min(length, capacity)`.
///
/// # Safety
/// `arr` is a live, forwarding-resolved array head; nothing here allocates.
#[inline]
pub(crate) unsafe fn ek_set_prefix(arr: *mut ArrayHeader, old_prefix: u32, proved: u32) {
    let mut prefix = proved;
    if prefix > old_prefix && super::array_has_element_descriptors_resolved(arr) {
        prefix = old_prefix;
    }
    (*arr).ek_prefix = prefix.min((*arr).length).min((*arr).capacity);
}

/// One element store at `index` with the stored `bits` (the funnel row of
/// §3.1). A hole below P lowers P to `index`. A non-hole store at the
/// frontier (`index == P`) inside the live range raises P by one unless the
/// array holds element descriptors. A store past the frontier leaves P (no
/// forward scan). Callers that write a slot before they publish the length
/// covering it call this again after the length update, where the frontier
/// rise applies.
///
/// # Safety
/// `arr` is a live, forwarding-resolved array head and the slot is written.
#[inline(always)]
pub(crate) unsafe fn ek_note_slot(arr: *mut ArrayHeader, index: usize, bits: u64) {
    let prefix = (*arr).ek_prefix as usize;
    if bits == crate::value::TAG_HOLE {
        if index < prefix {
            (*arr).ek_prefix = index as u32;
        }
    } else if index == prefix
        && index < ((*arr).length.min((*arr).capacity)) as usize
        && !super::array_has_element_descriptors_resolved(arr)
    {
        (*arr).ek_prefix = prefix as u32 + 1;
    }
    debug_assert!((*arr).ek_prefix <= (*arr).length.min((*arr).capacity));
}

/// The birth row (§3.1): the builder of a fresh, unpublished array states P
/// once, when its fill is done, from its own proof that every slot in
/// `[0, n)` holds a plain data element. The per-slot init helpers
/// (`note_array_slot_layout_only`, `fill_resolved_array_slot`) do no EK work:
/// the array they write is the builder's, and only the builder knows the
/// whole fill. A fresh array owns no named layout, so no slot below `n` has
/// an element descriptor.
///
/// # Safety
/// `arr` is a live, forwarding-resolved array head that no other code has
/// seen yet, and `[0, n)` holds no `TAG_HOLE`.
#[inline(always)]
pub(crate) unsafe fn ek_birth_prefix(arr: *mut ArrayHeader, n: u32) {
    debug_assert!(
        (*arr).named.is_null(),
        "EK: a born array owns no named layout"
    );
    debug_assert!(n <= (*arr).length.min((*arr).capacity));
    (*arr).ek_prefix = n.min((*arr).length).min((*arr).capacity);
    ek_debug_verify(arr);
}

/// The frontier row for an append that wrote slot `index` and then
/// published the length covering it (`push`, `append`): [`ek_note_slot`]
/// with the slot's current bits, now that the slot is live.
///
/// # Safety
/// `arr` is a live, forwarding-resolved array head and `index < length`.
#[inline(always)]
pub(crate) unsafe fn ek_note_published(arr: *mut ArrayHeader, index: usize) {
    if index == (*arr).ek_prefix as usize {
        ek_note_slot(arr, index, *super::array_elements_ptr(arr).add(index));
    }
}

/// An element descriptor install on an array (EK §3.1, the descriptor
/// row): every index key the edits give a data attribute or an accessor
/// leaves the prefix. Every descriptor install entry calls this with the
/// receiver (`descriptor_state::note_element_install`), so every define path
/// (`defineProperty`, `__defineGetter__`, attribute installs) lowers P
/// before a read can skip the descriptor. Removals and whole-list edits leave P alone: a cleared
/// descriptor makes no slot less plain, and integrity edits leave elements
/// readable as data.
///
/// # Safety
/// `owner` is the address the descriptor install was made for.
pub(crate) unsafe fn ek_note_descriptor_edits(
    owner: usize,
    edits: &[crate::object::descriptor_state::AttrsEdit<'_>],
) {
    use crate::object::descriptor_state::AttrsEdit;
    // The keys first: only an index key can touch the prefix, and most
    // installs are named (`__export` getters), so the owner is classified
    // only for an edit that gives an index a descriptor.
    let mut lowest: Option<u32> = None;
    for edit in edits {
        let key = match *edit {
            AttrsEdit::Data(key, _) | AttrsEdit::Accessor(key, _, _) => key,
            _ => continue,
        };
        if let Some(index) = crate::object::field_get_set::canonical_array_index_bytes(key) {
            lowest = Some(lowest.map_or(index, |low| low.min(index)));
        }
    }
    let Some(lowest) = lowest else {
        return;
    };
    let Some(header) = crate::value::addr_class::try_read_tracked_gc_header(owner) else {
        return;
    };
    if (*header.as_ptr()).obj_type != crate::gc::GC_TYPE_ARRAY {
        return;
    }
    let arr = super::clean_arr_ptr(owner as *const ArrayHeader) as *mut ArrayHeader;
    if arr.is_null() {
        return;
    }
    ek_lower_prefix(arr, lowest);
}

/// `splice(s, d, ...k)` (§3.1): `[0, s)` is untouched, `[s, s+k)` holds the
/// items, and the moved tail's leading non-hole run is old `[s+d, P)`.
#[inline]
pub(crate) fn ek_prefix_after_splice(prefix: u32, start: u32, deleted: u32, inserted: u32) -> u32 {
    if start > prefix {
        prefix
    } else {
        start + inserted + prefix.saturating_sub(start + deleted)
    }
}

/// `copyWithin(t, s, ...)` moving `count` slots from `s` to `t` (§3.1).
#[inline]
pub(crate) fn ek_prefix_after_copy_within(
    prefix: u32,
    target: u32,
    source: u32,
    count: u32,
) -> u32 {
    if target > prefix {
        return prefix;
    }
    let leading = prefix.saturating_sub(source).min(count);
    if leading == count {
        prefix.max(target + count)
    } else {
        target + leading
    }
}

/// The DEBUG word check (§5): the prefix claim against the contents, O(P).
/// Bulk operations (moves, truncation, fill, defines) run it on exit, where
/// it costs what the operation costs; a single read checks only its own slot
/// ([`ek_debug_verify_read`]). Release builds compile both away.
///
/// # Safety
/// `arr` is a live, forwarding-resolved array head.
#[inline(always)]
pub(crate) unsafe fn ek_debug_verify(arr: *const ArrayHeader) {
    #[cfg(debug_assertions)]
    ek_verify(arr);
    #[cfg(not(debug_assertions))]
    let _ = arr;
}

/// The DEBUG check at a runtime read entry: a read of slot `index` below P
/// relies on exactly this slot being a plain data element.
///
/// # Safety
/// `arr` is a live, forwarding-resolved array head.
#[inline(always)]
pub(crate) unsafe fn ek_debug_verify_read(arr: *const ArrayHeader, index: usize) {
    #[cfg(debug_assertions)]
    if index < (*arr).ek_prefix as usize {
        let prefix = (*arr).ek_prefix;
        assert!(
            prefix <= (*arr).length.min((*arr).capacity),
            "EK: prefix {prefix} past the live range"
        );
        assert!(
            *super::array_elements_ptr(arr).add(index) != crate::value::TAG_HOLE,
            "EK: read of slot {index} below prefix {prefix} finds a hole"
        );
        if super::array_has_element_descriptors_resolved(arr) {
            assert!(
                crate::object::get_accessor_descriptor(arr as usize, &index.to_string()).is_none(),
                "EK: read of slot {index} below prefix {prefix} finds an accessor"
            );
        }
    }
    #[cfg(not(debug_assertions))]
    let _ = (arr, index);
}

#[cfg(any(test, debug_assertions))]
#[inline(never)]
pub(crate) unsafe fn ek_verify(arr: *const ArrayHeader) {
    let prefix = (*arr).ek_prefix;
    assert!(
        prefix <= (*arr).length.min((*arr).capacity),
        "EK: prefix {prefix} exceeds min(length {}, capacity {})",
        (*arr).length,
        (*arr).capacity
    );
    if prefix == 0 {
        return;
    }
    let elements = super::array_elements_ptr(arr);
    for i in 0..prefix as usize {
        assert!(
            *elements.add(i) != crate::value::TAG_HOLE,
            "EK: slot {i} below prefix {prefix} is a hole"
        );
    }
    if super::array_has_element_descriptors_resolved(arr) {
        for i in 0..prefix {
            let key = i.to_string();
            // A read relies on the slot holding the value: an accessor is the
            // descriptor that breaks it. Attributes alone (`freeze`, `seal`)
            // leave the element data (§3.1 row `Object.freeze`).
            assert!(
                crate::object::get_accessor_descriptor(arr as usize, &key).is_none(),
                "EK: slot {i} below prefix {prefix} carries an accessor"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splice_prefix_follows_the_moved_slots() {
        // a = [1,2,3,·,·,·,7], P = 3: splice(0, 1) leaves [2,3,·,...]: P = 2.
        assert_eq!(ek_prefix_after_splice(3, 0, 1, 0), 2);
        // splice(1, 2, 9) on the same start: [1,9,·,...]: P = 2.
        assert_eq!(ek_prefix_after_splice(3, 1, 2, 1), 2);
        // Insertion inside the prefix: everything shifts up, P + k.
        assert_eq!(ek_prefix_after_splice(3, 1, 0, 2), 5);
        // Past the prefix nothing below P moves.
        assert_eq!(ek_prefix_after_splice(3, 5, 1, 4), 3);
    }

    #[test]
    fn copy_within_prefix_follows_the_moved_slots() {
        // a = [1,2,·,4], P = 2: copyWithin(0, 1) moves [2,·,4] to 0: P = 1.
        assert_eq!(ek_prefix_after_copy_within(2, 0, 1, 3), 1);
        // A source entirely below P keeps the target dense: max(P, t + c).
        assert_eq!(ek_prefix_after_copy_within(4, 2, 0, 2), 4);
        assert_eq!(ek_prefix_after_copy_within(4, 4, 0, 2), 6);
        // A target past the prefix leaves it.
        assert_eq!(ek_prefix_after_copy_within(2, 3, 0, 1), 2);
    }
}
