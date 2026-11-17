//! Shape-table housekeeping: shrinking after pruning, and the `PERRY_GC_CENSUS` rows.
use super::*;

/// #9612: release the capacity that pruning left behind.
///
/// hashbrown never shrinks on `remove`/`retain`, so the shape tables keep the
/// allocation of their startup PEAK for the life of the process. Measured on
/// the compiled claude-code TUI at idle: `ids_by_facts` held 30.8 MB at 12.3%
/// fill and `descriptors` 13.8 MB at 10.5% fill, i.e. sized for a peak that
/// `prune_dead_shape_keys` had already discarded.
///
/// Called once per MAJOR collection, right after the prune, where a rehash is
/// already amortized against a full heap walk. #9706: the by-id store is a
/// slab now, so this also releases its all-dead chunks; the family and slot
/// index maps are shrunk to `len + len / 4`, one growth step of headroom.
pub(crate) fn shrink_shape_tables() {
    fn worth_shrinking(len: usize, capacity: usize) -> bool {
        // Only when the table is holding real slack and is less than half
        // used; a small or well-packed table is left alone.
        capacity > 4096 && capacity > len.saturating_mul(2)
    }
    let table = &crate::state::state().shapes;
    let mut inner = table.inner.borrow_mut();
    if worth_shrinking(inner.owned_indices.len(), inner.owned_indices.capacity()) {
        let target = inner.owned_indices.len() + inner.owned_indices.len() / 4;
        inner.owned_indices.shrink_to(target);
    }
    if worth_shrinking(inner.by_facts.len(), inner.by_facts.capacity()) {
        let target = inner.by_facts.len() + inner.by_facts.len() / 4;
        inner.by_facts.shrink_to(target);
    }
    if worth_shrinking(inner.owned_families.len(), inner.owned_families.capacity()) {
        let target = inner.owned_families.len() + inner.owned_families.len() / 4;
        inner.owned_families.shrink_to(target);
    }
    // SAFETY: the prune that precedes this call holds no slab reference, and
    // neither does anything else while the major collection owns the agent.
    unsafe { table.slab_mut().release_empty_chunks() };
}

/// `PERRY_GC_CENSUS`: the by-id slab, the per-shape key indices, the
/// exact-facts accelerator and the keys-address family index.
pub(crate) fn shape_table_census() -> Vec<crate::gc::census::SideTableRow> {
    use crate::gc::census::map_bytes;
    let table = &crate::state::state().shapes;
    let inner = table.inner.borrow();
    let slab = table.slab();
    let mut rows = Vec::new();
    rows.push(("shapes.descriptors", slab.len(), slab.estimated_bytes()));
    let index_inner: usize = inner
        .owned_indices
        .values()
        .map(|ix| ix.slots.heap_bytes())
        .sum();
    rows.push((
        "shapes.owned_indices",
        inner.owned_indices.len(),
        map_bytes(&inner.owned_indices) + index_inner,
    ));
    // CKI S2: the families and slot indices canonical lists own on their
    // trie nodes (no map; the bytes are the lists' and boxes' heap).
    let (mut list_families, mut list_family_bytes, mut list_indices, mut list_index_bytes) =
        (0usize, 0usize, 0usize, 0usize);
    crate::object::canonical_keys::for_each_list_shapes(|_, _, list| {
        if !list.family.is_empty() {
            list_families += 1;
            list_family_bytes += list.family.heap_bytes();
        }
        if let Some(index) = &list.index {
            list_indices += 1;
            list_index_bytes += std::mem::size_of::<ShapeIndex>() + index.slots.heap_bytes();
        }
    });
    rows.push(("shapes.list_indices(node)", list_indices, list_index_bytes));
    let facts_inner: usize = inner.by_facts.values().map(IdList::heap_bytes).sum();
    rows.push((
        "shapes.by_facts",
        inner.by_facts.len(),
        map_bytes(&inner.by_facts) + facts_inner,
    ));
    let families_inner: usize = inner.owned_families.values().map(IdList::heap_bytes).sum();
    rows.push((
        "shapes.owned_families",
        inner.owned_families.len(),
        map_bytes(&inner.owned_families) + families_inner,
    ));
    rows.push((
        "shapes.list_families(node)",
        list_families,
        list_family_bytes,
    ));
    rows.push((
        "shapes.keyless_family",
        inner.keyless_family.len(),
        inner.keyless_family.heap_bytes(),
    ));
    // Ids ever minted by this process: the slab is indexed by id, so the gap
    // between this and `shapes.descriptors` is what chunk release reclaims.
    let minted = SHAPE_ID_NEXT
        .iter()
        .map(|next| next.load(std::sync::atomic::Ordering::Relaxed) - STATIC_SHAPE_ID_END)
        .sum::<u32>();
    rows.push(("shapes.ids_minted(process)", minted as usize, 0));
    // How the descriptor population splits by [[Prototype]] identity kind,
    // and how many distinct prototype identities it names: what the
    // prototype-in-shape rule costs in shapes.
    let mut by_kind = [0usize; 6];
    let mut extensions = (0, 0, 0, 0);
    let mut distinct = std::collections::HashSet::new();
    slab.for_each_handle(|_, record| {
        // SAFETY: live slab record, read immediately under agent ownership.
        let owned = unsafe { (*record).extension_census() };
        extensions.0 += owned.0;
        extensions.1 += owned.1;
        extensions.2 += owned.2;
        extensions.3 += owned.3;
        let proto_id = unsafe { (*record).proto_id };
        distinct.insert(proto_id);
        let kind = match proto_id {
            PROTO_ID_DEFAULT => 0,
            PROTO_ID_NULL => 1,
            id if id >> PROTO_ID_TAG_SHIFT == 0 => 2,
            id if id & PROTO_ID_UNIQUE == PROTO_ID_CLASS => 3,
            id if id & PROTO_ID_UNIQUE == PROTO_ID_MIXED => 4,
            _ => 5,
        };
        by_kind[kind] += 1;
    });
    for (name, count) in [
        "shapes.proto(object_prototype)",
        "shapes.proto(null)",
        "shapes.proto(object_serial)",
        "shapes.proto(class_default)",
        "shapes.proto(class_recorded)",
        "shapes.proto(unique)",
    ]
    .into_iter()
    .zip(by_kind)
    {
        rows.push((name, count, 0));
    }
    rows.push(("shapes.proto.distinct_identities", distinct.len(), 0));
    rows.push((
        "shapes.extensions(native owned)",
        extensions.0,
        extensions.1,
    ));
    // A subset of extension bytes, so this informational row adds no bytes
    // to the census total a second time.
    rows.push(("shapes.store_chain(facts)", extensions.2, 0));
    rows.push((
        "shapes.store_chain(bytes included in extensions)",
        extensions.3,
        0,
    ));
    rows
}

/// `PERRY_GC_CENSUS`: how the descriptor population relates to the live heap
/// (#9706). `live_ids` is the sorted, deduplicated set of ShapeIds stamped on
/// live shaped objects, collected by the census walk.
///
/// * `shapes.descriptors.carried` — descriptors some live object is stamped
///   with: the population V8's "object shape" bucket corresponds to.
/// * `shapes.descriptors.uncarried` — descriptors no live object carries:
///   transition history a cache may reinstall (`cache_carrier`), versions
///   kept for an old receiver since the last full trace, and shapes whose
///   keys array is still alive on some other descriptor.
/// * `shapes.families.multi` — keys arrays with more than one descriptor,
///   and the descriptors they hold beyond the first: the duplication the
///   family walk pays for.
pub(crate) fn shape_table_liveness_census(
    live_ids: &[LayoutWord],
) -> Vec<crate::gc::census::SideTableRow> {
    let table = &crate::state::state().shapes;
    let inner = table.inner.borrow();
    let slab = table.slab();
    let mut carried = 0usize;
    let mut uncarried = 0usize;
    let mut uncarried_cache = 0usize;
    let mut uncarried_old = 0usize;
    slab.for_each_handle(|id, record| {
        if live_ids.binary_search(&id).is_ok() {
            carried += 1;
            return;
        }
        uncarried += 1;
        // SAFETY: live slab record, read immediately.
        let record = unsafe { *record };
        if record.cache_carrier() {
            uncarried_cache += 1;
        } else if record.has(RECORD_FLAG_OLD_CARRIER) {
            uncarried_old += 1;
        }
    });
    let mut multi_families = 0usize;
    let mut multi_extra = 0usize;
    let mut largest = 0usize;
    let mut sizes: Vec<usize> = inner.owned_families.values().map(IdList::len).collect();
    sizes.push(inner.keyless_family.len());
    crate::object::canonical_keys::for_each_list_shapes(|_, _, list| {
        sizes.push(list.family.len());
    });
    for n in sizes {
        largest = largest.max(n);
        if n > 1 {
            multi_families += 1;
            multi_extra += n - 1;
        }
    }
    vec![
        ("shapes.descriptors.carried(live objects)", carried, 0),
        ("shapes.descriptors.uncarried", uncarried, 0),
        (
            "shapes.descriptors.uncarried.cache_carrier",
            uncarried_cache,
            0,
        ),
        ("shapes.descriptors.uncarried.old_carrier", uncarried_old, 0),
        (
            "shapes.families.multi(families,extra descriptors)",
            multi_families,
            multi_extra,
        ),
        ("shapes.families.largest", largest, 0),
    ]
}
