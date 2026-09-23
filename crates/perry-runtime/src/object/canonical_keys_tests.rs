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
        .map(|i| table.alloc_node(i, ROOT_NODE, i as u64, 1, true, true))
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
        .map(|i| table.alloc_node(i, ROOT_NODE, 7, 1, true, true))
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
    let parent = table.alloc_node(1, ROOT_NODE, 10, 1, true, true);
    let child = table.alloc_node(2, parent, 20, 2, true, true);
    let dead_child = table.alloc_node(3, parent, 20, 2, true, true);
    let sibling = table.alloc_node(4, parent, 20, 2, true, true);
    let grandchild = table.alloc_node(5, child, 30, 3, true, true);
    // Duplicate and invalid ids must not corrupt the free list or census.
    table.free_nodes(&[parent, dead_child, parent, ROOT_NODE, NO_NODE]);
    assert_eq!(table.reaped, 2);
    assert_eq!(table.edges.len(), 1);
    assert_eq!(table.edges[&(child, 30)], grandchild);
    for id in [child, sibling] {
        assert_eq!(table.nodes[id as usize].parent, NO_NODE);
        assert_eq!(table.nodes[id as usize].next, NO_NODE);
    }
    let reused_child = table.alloc_node(6, ROOT_NODE, 40, 1, true, true);
    let reused_parent = table.alloc_node(7, ROOT_NODE, 50, 1, true, true);
    assert_eq!((reused_child, reused_parent), (dead_child, parent));
    assert!(!table.edges.contains_key(&(reused_parent, 20)));
    assert_eq!(table.by_addr[&2], child);
    assert_eq!(table.by_addr[&4], sibling);
    table.free_nodes(&[child, sibling, grandchild, reused_child, reused_parent]);
    assert!(table.edges.is_empty());
    assert!(table.by_addr.is_empty());
}

/// A family with disjoint first edges has exactly F*N trie nodes. Only its
/// published leaves need arrays, not every unobserved fold prefix.
#[test]
fn published_family_allocates_linear_element_slots() {
    let _lock = crate::gc::global_side_table_test_lock();
    reset_for_test();
    unsafe {
        let proof = SharedLayout::shape_cache_entry();
        const F: usize = 4;
        const N: usize = 128;
        let scope = crate::gc::RuntimeHandleScope::new();
        for family in 0..F {
            let names: Vec<String> = (0..N).map(|i| format!("family_{family}_key_{i}")).collect();
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            let list = canonicalize(&proof, raw_list(&names), N as u32);
            scope.root_raw_mut_ptr(list.as_ptr());
            assert_eq!(list.len(), N as u32);
        }
        let slots = with_table_or(0, |t| t.allocated_slots);
        eprintln!(
            "published family: {F} leaves, {} nodes, {slots} allocated slots",
            F * N
        );
        assert!(
            slots <= (2 * F * N) as u64,
            "unpublished prefixes allocated quadratic storage: {slots}"
        );
    }
}

unsafe fn assert_prefix(list: crate::gc::RuntimeHandle<'_>) {
    list.with_const_ptr::<ArrayHeader, _>(|a| {
        assert_eq!((*a).length, 2);
        let (slots, len) = crate::object::keys_array_dense_slots(a);
        assert_eq!(len, 2);
        for (i, expected) in ["prefix_key_0", "prefix_key_1"].iter().enumerate() {
            let mut sso = [0; crate::value::SHORT_STRING_MAX_LEN];
            let value = JSValue::from_bits((*slots.add(i)).to_bits());
            assert_eq!(
                crate::string::js_string_key_bytes(value, &mut sso),
                Some(expected.as_bytes())
            );
        }
    });
}

/// Published full/prefix lists stay independent across all key mutation
/// families: ordinary append/define, dictionary append/grow, compact delete,
/// tombstone/squeeze, and mutation of the materialized reflection result.
#[test]
fn published_prefix_survives_object_and_reflection_writers() {
    let _lock = crate::gc::global_side_table_test_lock();
    reset_for_test();
    unsafe {
        for tombstones in [false, true] {
            let _mode = crate::object::delete_rest::test_scope_tombstone_deletes(tombstones);
            for dictionary in [false, true] {
                for count in [8, 40] {
                    let scope = crate::gc::RuntimeHandleScope::new();
                    let names: Vec<String> =
                        (0..count).map(|i| format!("prefix_key_{i}")).collect();
                    let names: Vec<&str> = names.iter().map(String::as_str).collect();
                    let full =
                        canonicalize(&SharedLayout::shape_cache_entry(), raw_list(&names), count);
                    let full = scope.root_raw_mut_ptr(full.as_ptr());
                    let prefix = full
                        .with_const_ptr(|a| canonicalize(&SharedLayout::shape_cache_entry(), a, 2));
                    let prefix = scope.root_raw_mut_ptr(prefix.as_ptr());
                    full.with_const_ptr::<ArrayHeader, _>(|a| {
                        prefix.with_const_ptr(|b| assert_ne!(a, b))
                    });
                    let object = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, count));
                    object.with_mut_ptr(|o| {
                        full.with_mut_ptr(|a| {
                            crate::object::set_object_keys_array_with_live(o, a, count)
                        })
                    });
                    assert_prefix(prefix);
                    if dictionary {
                        assert!(object.with_mut_ptr(|o| {
                            crate::object::dictionary::latch_object_to_dictionary(o)
                        }));
                        object.with_const_ptr(|o| {
                            full.with_mut_ptr(|a| {
                                assert_ne!(crate::object::object_keys_array(o), a)
                            })
                        });
                    }
                    // Both dictionary push/grow and ordinary canonical append.
                    for i in count..count + 12 {
                        let k = key(&format!("prefix_key_{i}"));
                        object.with_mut_ptr(|o| {
                            crate::object::js_object_set_field_by_name(o, k, i as f64)
                        });
                        assert_prefix(prefix);
                    }
                    let descriptor = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
                    for (name, value) in [
                        ("value", 17.0),
                        ("enumerable", f64::from_bits(crate::value::TAG_TRUE)),
                    ] {
                        let k = key(name);
                        descriptor.with_mut_ptr(|o| {
                            crate::object::js_object_set_field_by_name(o, k, value)
                        });
                    }
                    let defined = key("defined_key");
                    object.with_const_ptr::<crate::ObjectHeader, _>(|o| {
                        descriptor.with_const_ptr::<crate::ObjectHeader, _>(|d| {
                            crate::object::js_object_define_property(
                                crate::value::js_nanbox_pointer(o as i64),
                                crate::value::js_nanbox_string(defined as i64),
                                crate::value::js_nanbox_pointer(d as i64),
                            );
                        })
                    });
                    assert_prefix(prefix);
                    for i in 0..count {
                        let k = key(&format!("prefix_key_{i}"));
                        assert_eq!(
                            object.with_mut_ptr(|o| crate::object::js_object_delete_field(o, k)),
                            1
                        );
                        assert_prefix(prefix);
                    }
                    let result = object.with_const_ptr(|o| crate::object::js_object_keys(o));
                    prefix.with_mut_ptr(|a| assert_ne!(result, a));
                    let result = crate::array::js_array_sort_default(result);
                    crate::array::js_array_set(result, 0, JSValue::number(99.0));
                    let result = crate::array::js_array_push(result, JSValue::number(100.0));
                    crate::array::js_array_set_length(result, 0.0);
                    assert_prefix(prefix);
                    // The shape is the reflection authority; no descendant
                    // suffix may appear when the prefix is itself published.
                    let prefix_obj = crate::object::js_object_alloc(0, 2);
                    prefix.with_mut_ptr(|a| {
                        crate::object::set_object_keys_array_with_live(prefix_obj, a, 2)
                    });
                    let reflected =
                        scope.root_raw_mut_ptr(crate::object::js_object_keys(prefix_obj));
                    assert_prefix(reflected);
                }
            }
        }
    }
}

#[test]
fn published_prefix_survives_stable_sso_append_grow_and_squeeze() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _mode = crate::object::delete_rest::test_scope_tombstone_deletes(true);
    reset_for_test();
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let full = canonicalize(
            &SharedLayout::shape_cache_entry(),
            raw_list(&["prefix_key_0", "prefix_key_1", "prefix_key_2"]),
            3,
        );
        let full = scope.root_raw_mut_ptr(full.as_ptr());
        let prefix =
            full.with_const_ptr(|a| canonicalize(&SharedLayout::shape_cache_entry(), a, 2));
        let prefix = scope.root_raw_mut_ptr(prefix.as_ptr());
        let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 3));
        obj.with_mut_ptr(|o| {
            full.with_mut_ptr(|a| crate::object::set_object_keys_array_with_live(o, a, 3))
        });
        for i in 0..3 {
            let k = key(&format!("prefix_key_{i}"));
            assert_eq!(
                obj.with_mut_ptr(|o| crate::object::js_object_delete_field(o, k)),
                1
            );
        }
        assert_eq!(
            obj.with_const_ptr(|o| crate::object::shapes::object_shape_hole_count(o)),
            3
        );
        for n in 0..13 {
            let name = format!("k{n}");
            let sso = JSValue::try_short_string(name.as_bytes()).unwrap();
            let (_, moved, _) = obj
                .with_mut_ptr(|o| {
                    crate::object::try_readd_stable_tombstone(o, f64::from_bits(sso.bits()), 1.0)
                })
                .expect("must exercise the stable SSO append, including capacity growth");
            obj.set_raw_mut_ptr(moved);
            assert_prefix(prefix);
            assert_eq!(
                obj.with_mut_ptr(|o| crate::object::js_object_delete_dynamic(
                    o,
                    f64::from_bits(sso.bits())
                )),
                1
            );
            assert_prefix(prefix);
        }
        obj.with_const_ptr(|o| {
            assert_eq!(
                crate::array::js_array_length(crate::object::object_keys_array(o)),
                0
            );
            assert_eq!(crate::object::shapes::object_shape_hole_count(o), 0);
        });
        let k = key("heap_key_after_squeeze");
        assert!(obj
            .with_mut_ptr(|o| crate::object::try_readd_stable_tombstone(
                o,
                crate::value::js_nanbox_string(k as i64),
                2.0
            ))
            .is_some());
        assert_prefix(prefix);
    }
}

/// A metadata prefix's witness and its eventual publication can use different
/// representations of equal key bytes. GC layout follows the actual storage.
#[test]
fn publishing_prefix_refreshes_pointer_layout() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        for witness_sso in [false, true] {
            reset_for_test();
            let proof = SharedLayout::shape_cache_entry();
            let make = |sso: bool, names: &[&str]| {
                let mut a = crate::array::js_array_alloc(names.len() as u32);
                for name in names {
                    let v = if sso {
                        JSValue::try_short_string(name.as_bytes()).unwrap()
                    } else {
                        JSValue::string_ptr(key(name))
                    };
                    a = crate::array::js_array_push(a, v);
                }
                a
            };
            let full = canonicalize(&proof, make(witness_sso, &["a", "b"]), 2);
            let prefix = canonicalize(&proof, make(!witness_sso, &["a"]), 1);
            assert_ne!(full.addr(), prefix.addr());
            with_table_or((), |t| {
                let id = node_of(t, prefix).unwrap();
                assert_eq!(t.nodes[id as usize].all_ptr, witness_sso);
            });
            let fork = extend_key(&proof, prefix, key("c"));
            with_table_or((), |t| {
                let id = node_of(t, fork).unwrap();
                assert_eq!(t.nodes[id as usize].all_ptr, witness_sso);
            });
        }
    }
}

/// #10969: a list grown key by key hands its slot index to each successor.
///
/// Every length of a growing list is its own canonical array. Without the
/// carry, the find-before-append of each set indexed the whole current list
/// again, so an N-key object built key by key indexed O(N^2) slots —
/// 2,199,657 on `ts.transpileModule` against 75,775 before canonical
/// identity. With it, the index moves along the chain: only the newest list
/// holds one, and it covers the whole list.
#[test]
fn a_growing_list_hands_its_slot_index_to_each_successor() {
    let _lock = crate::gc::global_side_table_test_lock();
    reset_for_test();
    // Nothing may be pruned while the chain is inspected.
    let _no_gc = crate::gc::GcSuppressScope::new();
    let scope = crate::gc::RuntimeHandleScope::new();
    const KEY_COUNT: usize = 96;
    unsafe {
        let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        let mut lists = Vec::with_capacity(KEY_COUNT);
        for i in 0..KEY_COUNT {
            let name = format!("carry_{i}");
            obj.with_mut_ptr(|o| {
                crate::object::js_object_set_field_by_name(o, key(&name), i as f64)
            });
            lists.push(obj.with_const_ptr(|o| crate::object::object_keys_array(o)));
        }
        let distinct: std::collections::HashSet<_> = lists.iter().copied().collect();
        assert_eq!(
            distinct.len(),
            KEY_COUNT,
            "premise: every length is its own canonical list"
        );
        let holders: Vec<(usize, u32)> = lists[..KEY_COUNT - 1]
            .iter()
            .enumerate()
            .filter_map(|(i, list)| {
                crate::object::shapes::test_keys_index_len(*list).map(|n| (i + 1, n))
            })
            .collect();
        assert!(
            holders.is_empty(),
            "INVARIANT: an earlier length must not keep a slot index of its own; {} of \
             them do (length, indexed): {:?}",
            holders.len(),
            &holders[..holders.len().min(6)]
        );
        assert_eq!(
            crate::object::shapes::test_keys_index_len(lists[KEY_COUNT - 1]),
            Some(KEY_COUNT as u32),
            "INVARIANT: the newest list carries a complete slot index"
        );
    }
}
