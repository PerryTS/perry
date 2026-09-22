use super::*;

// Test instrumentation only: one count per candidate slot read by probe().
// No declaration, read or branch survives in a production build.
crate::perry_thread_local! {
    static SLOT_READS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

pub(super) fn note_slot_read() {
    SLOT_READS.with(|c| c.set(c.get() + 1));
}

fn key(name: &str) -> *mut StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

/// A keys array built the way a producer that has NOT been funnelled
/// would build one: its own allocation, its own address.
unsafe fn raw_list(names: &[&str]) -> *mut ArrayHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr = scope.root_raw_mut_ptr(crate::array::js_array_alloc(names.len() as u32 + 4));
    let (_, result) = arr.across_mut(|| {
        for name in names {
            let k = key(name);
            let grown = arr.with_mut_ptr(|a| {
                crate::array::js_array_push_f64(a, crate::value::js_nanbox_string(k as i64))
            });
            arr.set_raw_mut_ptr(grown);
        }
    });
    result
}

/// The whole of stage 1b in one assertion: separately allocated arrays
/// holding the same ordered list come back as ONE address, which is what
/// makes `facts_key`'s address term a content term with no edit to
/// `facts_key` at all.
#[test]
fn one_array_serves_one_ordered_key_list() {
    let _lock = crate::gc::global_side_table_test_lock();
    reset_for_test();
    unsafe {
        let a = raw_list(&["alpha", "beta", "gamma"]);
        let b = raw_list(&["alpha", "beta", "gamma"]);
        assert_ne!(a, b, "test premise: two separately allocated arrays");
        let proof = SharedLayout::shape_cache_entry();
        let ca = canonicalize(&proof, a, 3);
        let cb = canonicalize(&proof, b, 3);
        assert_eq!(
            ca.addr(),
            cb.addr(),
            "same ordered list, two arrays -- canonicalization must give one address"
        );
        assert_eq!(ca.len(), 3);
        // And the grow path reaches the same node as the whole-list form.
        let grown = extend_key(
            &proof,
            extend_key(
                &proof,
                extend_key(&proof, CanonicalKeys::EMPTY, key("alpha")),
                key("beta"),
            ),
            key("gamma"),
        );
        assert_eq!(
            grown.addr(),
            ca.addr(),
            "extend_slot and canonicalize must reach the same node, or they are two paths"
        );
    }
}

/// The must-fail control of L8.3.15, as a test rather than a promise: a
/// canonicalization that sorted or otherwise reordered keys would be a
/// silent WRONG ANSWER in every program, not a slow one.
#[test]
fn key_order_is_part_of_the_identity() {
    let _lock = crate::gc::global_side_table_test_lock();
    reset_for_test();
    unsafe {
        let proof = SharedLayout::shape_cache_entry();
        let ab = canonicalize(&proof, raw_list(&["a", "b"]), 2);
        let ba = canonicalize(&proof, raw_list(&["b", "a"]), 2);
        assert_ne!(
            ab.addr(),
            ba.addr(),
            "{{a,b}} and {{b,a}} are different layouts -- merging them is a wrong answer"
        );
    }
}

/// Per-prefix canonicalization, which is the correction of L8.3.15b: a
/// prefix is its own node, so `{a}` and `{a,b}` never share an address
/// and a `key_count` mint — 19,923 of tsc's 42,097 — cannot arise.
#[test]
fn a_prefix_is_its_own_node() {
    let _lock = crate::gc::global_side_table_test_lock();
    reset_for_test();
    unsafe {
        let full = raw_list(&["a", "b", "c"]);
        let proof = SharedLayout::shape_cache_entry();
        let two = canonicalize(&proof, full, 2);
        let three = canonicalize(&proof, full, 3);
        assert_eq!(
            two.len(),
            2,
            "the prefix array is exactly as long as its list"
        );
        assert_eq!(three.len(), 3);
        assert_ne!(two.addr(), three.addr());
        let one = canonicalize(&proof, full, 1);
        assert_eq!(one.len(), 1);
        assert_ne!(one.addr(), two.addr());
    }
}

/// L8.3.15's sixth prediction, measured rather than inspected: a probe
/// reads the ONE appended slot, however long the list already is. A
/// content-hash intern table would read N.
#[test]
fn a_probe_reads_one_slot_however_long_the_list() {
    let _lock = crate::gc::global_side_table_test_lock();
    reset_for_test();
    unsafe {
        let names: Vec<String> = (0..40).map(|i| format!("k{i}")).collect();
        let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        let proof = SharedLayout::shape_cache_entry();
        let long = canonicalize(&proof, raw_list(&refs), 40);
        assert_eq!(long.len(), 40);
        let tail_key = key("k40");
        // Publish the edge, then measure the HIT.
        let first = extend_key(&proof, long, tail_key);
        SLOT_READS.with(|c| c.set(0));
        let again = extend_key(&proof, long, tail_key);
        let reads = SLOT_READS.with(|c| c.get());
        assert_eq!(
            first.addr(),
            again.addr(),
            "the second extend_slot must hit"
        );
        assert!(
            reads <= 1,
            "a hit on a 41-key list read {reads} slots; the probe is not O(1)"
        );
    }
}

/// A tombstone is part of the ordered list, so its POSITION is part of
/// the identity. Two lists that differ only in where the hole sits must
/// not share a node.
#[test]
fn a_tombstone_position_is_part_of_the_identity() {
    let _lock = crate::gc::global_side_table_test_lock();
    reset_for_test();
    unsafe {
        let hole = JSValue::from_bits(crate::value::TAG_HOLE);
        let a = key("a");
        let b = key("b");
        let p = SharedLayout::shape_cache_entry();
        let hole_first = extend_slot(
            &p,
            extend_slot(
                &p,
                extend_slot(&p, CanonicalKeys::EMPTY, Appended::Slot(hole)),
                Appended::Key(a),
            ),
            Appended::Key(b),
        );
        let hole_middle = extend_slot(
            &p,
            extend_slot(
                &p,
                extend_slot(&p, CanonicalKeys::EMPTY, Appended::Key(a)),
                Appended::Slot(hole),
            ),
            Appended::Key(b),
        );
        let hole_first_again = extend_slot(
            &p,
            extend_slot(
                &p,
                extend_slot(&p, CanonicalKeys::EMPTY, Appended::Slot(hole)),
                Appended::Key(a),
            ),
            Appended::Key(b),
        );
        assert_eq!(
            hole_first.addr(),
            hole_first_again.addr(),
            "the same tombstone list must hit the same canonical node"
        );
        let raw = crate::array::js_array_alloc(3);
        let raw = crate::array::js_array_push(raw, hole);
        let raw = crate::array::js_array_push(raw, JSValue::string_ptr(a as *mut _));
        let raw = crate::array::js_array_push(raw, JSValue::string_ptr(b as *mut _));
        assert_eq!(
            canonicalize(&p, raw, 3).addr(),
            hole_first.addr(),
            "canonicalizing raw keys must preserve tombstones"
        );
        assert_eq!(hole_first.len(), 3);
        assert_eq!(hole_middle.len(), 3);
        assert_ne!(
            hole_first.addr(),
            hole_middle.addr(),
            "the tombstone position distinguishes two layouts"
        );
    }
}

/// Every canonical array is shared from birth (L8.3.15c), which is what
/// collapses every `keys_owned` branch to the shared arm rather than
/// leaving an arm that is merely unreached.
#[test]
fn every_canonical_array_is_shape_shared_from_birth() {
    let _lock = crate::gc::global_side_table_test_lock();
    reset_for_test();
    unsafe {
        let c = extend_key(
            &SharedLayout::shape_cache_entry(),
            CanonicalKeys::EMPTY,
            key("only"),
        );
        let gc = crate::value::addr_class::try_read_tracked_gc_header(c.addr())
            .expect("canonical keys must have a tracked header");
        assert!(
            (*gc.as_ptr()).gc_flags & crate::gc::GC_FLAG_SHAPE_SHARED != 0,
            "a canonical array that is not SHAPE_SHARED can be mutated in place"
        );
    }
}

/// Exercise retirement without heap allocations: these opaque address-index
/// tokens are never dereferenced. Count actual edge visits, not elapsed time.
#[test]
fn batch_prune_examines_linear_edges() {
    let _lock = crate::gc::global_side_table_test_lock();
    const N: usize = 20_000;
    const K: usize = 15_000;
    let mut table = CanonicalTable::new();
    let ids: Vec<u32> = (1..=N)
        .map(|i| table.alloc_node(i, ROOT_NODE, i as u64, 1, true))
        .collect();
    table.edge_examinations = 0;
    table.free_nodes(&ids[..K]);
    let examined = table.edge_examinations;
    eprintln!("prune n={N} k={K} edge_examinations={examined}");
    assert_eq!(table.by_addr.len(), N - K);
    assert_eq!(table.edges.len(), N - K);
    assert_eq!(table.reaped, K as u64);
    assert!(examined > 0, "the complexity counter must observe pruning");
    assert!(
        examined <= 4 * (N + K),
        "prune examined {examined} edges for n={N}, k={K}; expected O(n + k)"
    );
    // Return the census contributions too; no local fixture outlives this test.
    table.free_nodes(&ids[K..]);
}

/// All candidates share a hash: pruning must filter head, middle and tail
/// in one walk even when the caller's death order is the reverse of the chain.
#[test]
fn batch_prune_filters_collision_chains_once() {
    let _lock = crate::gc::global_side_table_test_lock();
    const N: usize = 20_000;
    let mut table = CanonicalTable::new();
    let ids: Vec<u32> = (1..=N)
        .map(|i| table.alloc_node(i, ROOT_NODE, 7, 1, true))
        .collect();
    let dead: Vec<u32> = ids
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 4 != 1)
        .map(|(_, &id)| id)
        .collect();
    table.edge_examinations = 0;
    table.free_nodes(&dead);
    assert!(table.edge_examinations <= 2 * N);
    let mut cur = table.edges[&(ROOT_NODE, 7)];
    for &id in ids
        .iter()
        .enumerate()
        .rev()
        .filter(|(i, _)| i % 4 == 1)
        .map(|(_, id)| id)
    {
        assert_eq!(cur, id);
        cur = table.nodes[cur as usize].next;
    }
    assert_eq!(cur, NO_NODE);
    table.free_nodes(&ids);
    assert!(table.edges.is_empty());
}

#[test]
fn batch_prune_orphans_children_before_reusing_ids() {
    let _lock = crate::gc::global_side_table_test_lock();
    let mut table = CanonicalTable::new();
    let parent = table.alloc_node(1, ROOT_NODE, 10, 1, true);
    let child = table.alloc_node(2, parent, 20, 2, true);
    let dead_child = table.alloc_node(3, parent, 20, 2, true);
    let sibling = table.alloc_node(4, parent, 20, 2, true);
    let grandchild = table.alloc_node(5, child, 30, 3, true);
    // Duplicate and invalid ids must not corrupt the free list or census.
    table.free_nodes(&[parent, dead_child, parent, ROOT_NODE, NO_NODE]);
    assert_eq!(table.reaped, 2);
    assert_eq!(table.edges.len(), 1);
    assert_eq!(table.edges[&(child, 30)], grandchild);
    for id in [child, sibling] {
        assert_eq!(table.nodes[id as usize].parent, NO_NODE);
        assert_eq!(table.nodes[id as usize].next, NO_NODE);
    }
    let reused_child = table.alloc_node(6, ROOT_NODE, 40, 1, true);
    let reused_parent = table.alloc_node(7, ROOT_NODE, 50, 1, true);
    assert_eq!((reused_child, reused_parent), (dead_child, parent));
    assert!(!table.edges.contains_key(&(reused_parent, 20)));
    assert_eq!(table.by_addr[&2], child);
    assert_eq!(table.by_addr[&4], sibling);
    table.free_nodes(&[child, sibling, grandchild, reused_child, reused_parent]);
    assert!(table.edges.is_empty());
    assert!(table.by_addr.is_empty());
}
