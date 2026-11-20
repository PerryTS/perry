//! The canonical table's census (live/minted/reaped nodes, published lists and
//! their storage, trie entries) plus the `#[cfg(test)]` hooks the suites use to
//! read and reset the table. Nothing here is on a hot path.
use super::*;

/// `(live nodes, nodes ever minted, nodes reaped)`. The census reads this;
/// nothing branches on it.
pub(crate) fn canonical_stats() -> (usize, u64, u64) {
    (
        CANON_LIVE.load(std::sync::atomic::Ordering::Relaxed) as usize,
        CANON_MINTED.load(std::sync::atomic::Ordering::Relaxed),
        CANON_REAPED.load(std::sync::atomic::Ordering::Relaxed),
    )
}

/// Total key count of live published lists — what the lists NAME. With
/// shared backings this exceeds the storage; see [`canonical_storage_stats`].
pub(crate) fn canonical_element_words() -> u64 {
    CANON_WORDS.load(std::sync::atomic::Ordering::Relaxed)
}

/// `(live published lists, live backing arrays, their element capacity,
/// in-place tip appends ever)`. The census reads this; nothing branches on it.
pub(crate) fn canonical_storage_stats() -> (u64, u64, u64, u64) {
    (
        CANON_PUBLISHED.load(std::sync::atomic::Ordering::Relaxed),
        CANON_BACKINGS.load(std::sync::atomic::Ordering::Relaxed),
        CANON_BACKING_SLOTS.load(std::sync::atomic::Ordering::Relaxed),
        CANON_IN_PLACE_APPENDS.load(std::sync::atomic::Ordering::Relaxed),
    )
}

/// Agent-local trie entries and reserved payload bytes for the exit census.
/// Hash-table control bytes and allocator overhead are excluded; backing
/// element storage is reported separately by `canonical_storage_stats`.
pub(crate) fn canonical_trie_stats() -> (usize, usize, usize) {
    with_table_or((0, 0, 0), |t| {
        let bytes = t.nodes.capacity() * std::mem::size_of::<Node>()
            + t.edges.capacity() * std::mem::size_of::<((u32, u64), u32)>();
        let published = t
            .nodes
            .iter()
            .filter(|n| n.published && n.addr != 0)
            .count();
        (t.edges.len(), published, bytes)
    })
}

/// Every published canonical list in this agent's trie, as an array address.
#[cfg(test)]
pub(crate) fn published_lists_for_test() -> Vec<*mut ArrayHeader> {
    with_table_or(Vec::new(), |t| {
        t.nodes
            .iter()
            .filter(|node| node.published && node.addr != 0)
            .map(|node| node.addr as *mut ArrayHeader)
            .collect()
    })
}

/// Does the node `keys` carries publish exactly `keys` in this agent's trie?
#[cfg(test)]
pub(crate) fn node_publishes_for_test(keys: crate::object::ObjectKeys) -> bool {
    with_table_or(false, |t| {
        validated_node(t, keys.arr(), keys.count(), keys.node()).is_some()
    })
}

#[cfg(test)]
pub(crate) fn reset_for_test() {
    let _ = CANONICAL_KEYS.try_with(|t| *t.borrow_mut() = CanonicalTable::new());
}
