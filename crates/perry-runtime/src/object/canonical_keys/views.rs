//! Reading the canonical table from outside: the shape-list views a published
//! key backs, the latched-away marker, and the published-node predicates the GC
//! and shape code ask.
use super::*;

/// A receiver latched to dictionary mode from `keys`: the next receiver to
/// arrive there is following it, so the lineage is not unique past it.
pub(crate) fn note_latched_away(keys: crate::object::ObjectKeys) {
    try_with_table(|t| {
        if let Some(id) = validated_node(t, keys.arr(), keys.count(), keys.node()) {
            t.note_reached(id);
        }
    });
}

/// Run `f` on the shape state of list `node` (CKI S2): its descriptor
/// family. `None` when the table is torn down or `node` names no list (a free
/// slot). The root is always a list: the keyless family lives on it even
/// after its optional empty backing died.
pub(crate) fn with_list_shapes<R>(
    node: u32,
    f: impl FnOnce(&mut crate::object::shapes::ListShapes) -> R,
) -> Option<R> {
    try_with_table(|t| {
        let n = t.nodes.get(node as usize)?;
        if node != ROOT_NODE && n.addr == 0 {
            return None;
        }
        let entry = &mut t.shapes[node as usize];
        let out = f(&mut entry.list);
        if entry.owns_heap() {
            t.shapes.note_heap(node as usize);
        }
        Some(out)
    })
    .flatten()
}

/// Run `f` on the shape state of the node that owns the backing of the list
/// `(arr, count)` whose holder carries `node`: the home of that backing's
/// slot index. `None` unless `node` still publishes exactly that list, so a
/// stale node can never answer for another backing's slots.
pub(crate) fn with_backing_shapes<R>(
    arr: *const ArrayHeader,
    count: u32,
    node: u32,
    f: impl FnOnce(&mut crate::object::shapes::ListShapes) -> R,
) -> Option<R> {
    try_with_table(|t| {
        // A carried node is trusted (the `CanonicalKeys` invariant); the
        // owner must still hold this very backing, which also refuses a
        // node whose list is not `arr`'s.
        let n = t.nodes.get(node as usize)?;
        debug_assert!(validated_node(t, arr, count, node).is_some());
        let _ = n;
        let owner = match t.shapes[node as usize].backing_owner {
            NO_NODE => node,
            owner => owner,
        };
        if t.nodes.get(owner as usize)?.addr != arr as usize {
            return None;
        }
        let entry = &mut t.shapes[owner as usize];
        let out = f(&mut entry.list);
        if entry.owns_heap() {
            t.shapes.note_heap(owner as usize);
        }
        Some(out)
    })
    .flatten()
}

/// Every list's shape state with its backing address, in node order. The
/// shape table's collector walk and census read families through this; `f`
/// must not reach the trie.
pub(crate) fn for_each_list_shapes(mut f: impl FnMut(u32, usize, &mut crate::object::shapes::ListShapes)) {
    try_with_table(|t| {
        for id in 0..t.nodes.len() {
            let addr = t.nodes[id].addr;
            if id == ROOT_NODE as usize || addr != 0 {
                let entry = &mut t.shapes[id];
                f(id as u32, addr, &mut entry.list);
                if entry.owns_heap() {
                    t.shapes.note_heap(id);
                }
            }
        }
    });
}

/// The node that publishes exactly the list `(arr, count)`, found from the
/// list's content, or `NO_NODE` when no node publishes it (an owned array, an
/// unlinked image cell, a shared copy of a canonical list). The shape mint
/// asks this only for a holder that brought no node: a carried node is the
/// answer, and its holders are expected to carry it.
///
/// Out of line and named, so a profile counts exactly the node-less mints.
///
/// # Safety
/// `arr` is a live keys array with at least `count` slots, or null.
#[inline(never)]
pub(crate) unsafe fn published_node_of(arr: *const ArrayHeader, count: u32) -> u32 {
    if arr.is_null() {
        return if count == 0 { ROOT_NODE } else { NO_NODE };
    }
    let shared = crate::value::addr_class::realm::is_image_object(arr as usize)
        || crate::value::addr_class::try_read_tracked_gc_header(arr as usize)
            .is_some_and(|gc| (*gc.as_ptr()).gc_flags & crate::gc::GC_FLAG_SHAPE_SHARED != 0);
    if !shared {
        // An owned list: it grows in place, so no node can name it.
        return NO_NODE;
    }
    with_table_or(NO_NODE, |t| {
        let (slots, available) = crate::object::keys_array_dense_slots(arr);
        if available < count as usize {
            return NO_NODE;
        }
        let mut parent = ROOT_NODE;
        for i in 0..count {
            let slot = Appended::Slot(JSValue::from_bits((*slots.add(i as usize)).to_bits()));
            let entry =
                crate::object::key_attrs::token_of_word(crate::object::key_attrs::key_word(arr, i));
            match probe_node(t, parent, i, slot, entry, slot.edge_hash(entry)) {
                Some(id) => parent = id,
                None => return NO_NODE,
            }
        }
        validated_node(t, arr, count, parent).unwrap_or(NO_NODE)
    })
}

/// Does `node` publish exactly `(arr, count)`? One node read: the check a
/// carried node passes before the shape mint takes it as the list's identity.
#[inline]
pub(crate) fn node_publishes(arr: *const ArrayHeader, count: u32, node: u32) -> bool {
    if node == ROOT_NODE && count == 0 {
        return true;
    }
    // A shared borrow: a read never writes the borrow flag's exclusive state.
    CANONICAL_KEYS
        .try_with(|t| {
            t.try_borrow()
                .is_ok_and(|t| validated_node(&t, arr, count, node).is_some())
        })
        .unwrap_or(false)
}
