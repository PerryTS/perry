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

#[test]
fn born_layout_uses_owned_keys_after_weak_cache_clear_and_collection() {
    let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    reset_for_test();
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let proof = SharedLayout::shape_cache_entry();
        let entries = [("name", 1.0), ("length", 2.0), ("prototype", 3.0)];
        let attrs = [0, crate::object::key_attrs::ENTRY_NON_WRITABLE, 0];
        let mut keys = CanonicalKeys::EMPTY;
        for (i, (name, _)) in entries.iter().enumerate() {
            keys = extend_key_with_entry(&proof, keys, key(name), attrs[i]);
        }
        assert!(crate::arena::pointer_in_old_gen(keys.addr()));
        let backing = scope.root_raw_mut_ptr(keys.as_ptr());
        crate::string::discard_weak_interns();
        assert!(crate::string::intern_lookup_bytes(b"name").is_none());
        crate::gc::js_gc_collect();
        let _no_gc = crate::gc::GcSuppressScope::new();
        let owned = backing.get_raw_mut_ptr::<ArrayHeader>();
        assert_eq!(
            owned as usize,
            keys.addr(),
            "canonical backing stays stationary"
        );
        let found = probe_born_layout(&proof, &entries, &attrs).expect("live owner, empty cache");
        assert_eq!(found.as_ptr(), owned);
        assert_eq!(found.len(), 3);
        assert!(probe_born_layout(&proof, &entries, &[0, 0, 0]).is_none());
        let reordered = [entries[1], entries[0], entries[2]];
        assert!(probe_born_layout(&proof, &reordered, &attrs).is_none());
        let other = [("name", 1.0), ("length", 2.0), ("prototype-other", 3.0)];
        assert!(probe_born_layout(&proof, &other, &attrs).is_none());
    }
}

#[test]
fn image_adoption_preserves_shared_prefix_and_gc_layout() {
    let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    reset_for_test();
    #[repr(C, align(8))]
    struct Image {
        gc: crate::gc::GcHeader,
        string: StringHeader,
        bytes: [u8; 10],
    }
    static IMAGE: Image = Image {
        gc: crate::gc::GcHeader {
            obj_type: crate::gc::GC_TYPE_STRING,
            gc_flags: crate::gc::GC_FLAG_MARKED
                | crate::gc::GC_FLAG_PINNED
                | crate::gc::GC_FLAG_INTERNED,
            _reserved: 0x494d,
            size: 38,
        },
        string: StringHeader {
            utf16_len: 9,
            byte_len: 9,
            capacity: 9,
            refcount: 0,
            flags: crate::string::STRING_FLAG_WTF8_VALIDATED,
        },
        bytes: *b"site-name\0",
    };
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let proof = SharedLayout::shape_cache_entry();
        let prefix = extend_key(&proof, CanonicalKeys::EMPTY, key("site-name"));
        let leaf = extend_key(&proof, prefix, key("tail"));
        assert_eq!(prefix.addr(), leaf.addr(), "premise: one shared backing");
        assert!(crate::arena::pointer_in_old_gen(leaf.addr()));
        let backing = scope.root_raw_mut_ptr(leaf.addr() as *mut ArrayHeader);
        let image = &IMAGE.string as *const StringHeader;
        backing.with_mut_ptr(|a| {
            let (slots, _) = crate::object::keys_array_dense_slots_resolved(a);
            assert_ne!(
                (*slots.add(0)).to_bits(),
                JSValue::string_ptr(image as *mut _).bits()
            );
            adopt_image_key(a, 0, image);
        });
        crate::gc::js_gc_collect();
        backing.with_const_ptr(|a: *const ArrayHeader| {
            assert_eq!(
                a as usize,
                leaf.addr(),
                "image adoption preserves stationary backing"
            );
            let (slots, len) = crate::object::keys_array_dense_slots_resolved(a as *mut _);
            assert_eq!(len, 2);
            assert_eq!(
                (*slots.add(0)).to_bits(),
                JSValue::string_ptr(image as *mut _).bits()
            );
            let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
            assert_eq!(
                crate::string::js_string_key_bytes(
                    JSValue::from_bits((*slots.add(1)).to_bits()),
                    &mut scratch,
                ),
                Some(b"tail".as_slice()),
            );
        });
    }
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
                extend_slot(&p, CanonicalKeys::EMPTY, Appended::Slot(hole), 0),
                Appended::Key(a),
                0,
            ),
            Appended::Key(b),
            0,
        );
        let hole_middle = extend_slot(
            &p,
            extend_slot(
                &p,
                extend_slot(&p, CanonicalKeys::EMPTY, Appended::Key(a), 0),
                Appended::Slot(hole),
                0,
            ),
            Appended::Key(b),
            0,
        );
        let hole_first_again = extend_slot(
            &p,
            extend_slot(
                &p,
                extend_slot(&p, CanonicalKeys::EMPTY, Appended::Slot(hole), 0),
                Appended::Key(a),
                0,
            ),
            Appended::Key(b),
            0,
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
    assert_eq!(table.published_count(), N - K);
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
    // A live child keeps its own identity: its node still validates for
    // exactly its `(backing, count)`, and a reused id never answers for it.
    assert_eq!(
        validated_node(&table, 2 as *const ArrayHeader, 2, child),
        Some(child)
    );
    assert_eq!(
        validated_node(&table, 4 as *const ArrayHeader, 2, sibling),
        Some(sibling)
    );
    assert_eq!(
        validated_node(&table, 3 as *const ArrayHeader, 2, reused_child),
        None
    );
    assert_eq!(
        validated_node(&table, 1 as *const ArrayHeader, 1, reused_parent),
        None
    );
    table.free_nodes(&[child, sibling, grandchild, reused_child, reused_parent]);
    assert!(table.edges.is_empty());
    assert_eq!(table.published_count(), 0);
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
                            crate::object::set_object_keys_with_live(
                                o,
                                crate::object::ObjectKeys::owned(a),
                                count,
                            )
                        })
                    });
                    assert_prefix(prefix);
                    if dictionary {
                        assert!(object
                            .with_mut_ptr(|o| { crate::object::overflow::establish_order(o) }));
                        object.with_const_ptr(|o| {
                            full.with_mut_ptr(|a| {
                                assert_ne!(crate::object::object_keys(o).arr(), a)
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
                        crate::object::set_object_keys_with_live(
                            prefix_obj,
                            crate::object::ObjectKeys::owned(a),
                            2,
                        )
                    });
                    let reflected =
                        scope.root_raw_mut_ptr(crate::object::js_object_keys(prefix_obj));
                    assert_prefix(reflected);
                }
            }
        }
    }
}

/// A published canonical prefix survives every writer on an object holding
/// the full list. Object model step 4: deletes, and the computed-key adds and
/// deletes that follow, go through the object's overflow (§3.4), which never
/// writes the shared list.
#[test]
fn published_prefix_survives_deletes_and_overflow_appends() {
    let _lock = crate::gc::global_side_table_test_lock();
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
            full.with_mut_ptr(|a| {
                crate::object::set_object_keys_with_live(o, crate::object::ObjectKeys::owned(a), 3)
            })
        });
        for i in 0..3 {
            let k = key(&format!("prefix_key_{i}"));
            assert_eq!(
                obj.with_mut_ptr(|o| crate::object::js_object_delete_field(o, k)),
                1
            );
            assert_prefix(prefix);
        }
        assert!(obj.with_const_ptr(|o| crate::object::overflow::has_overflow(o)));
        let target = || {
            obj.with_mut_ptr(|o: *mut crate::ObjectHeader| {
                crate::value::js_nanbox_pointer(o as i64)
            })
        };
        for n in 0..13 {
            let name = format!("k{n}");
            let sso = JSValue::try_short_string(name.as_bytes()).unwrap();
            crate::object::js_object_set_property_key(target(), f64::from_bits(sso.bits()), 1.0);
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
        obj.with_mut_ptr(|o| {
            assert_eq!(
                crate::array::js_array_length(crate::object::js_object_keys(o)),
                0
            );
        });
        let k = key("heap_key_after_overflow");
        crate::object::js_object_set_property_key(
            target(),
            crate::value::js_nanbox_string(k as i64),
            2.0,
        );
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

#[test]
fn empty_root_backing_is_shared_weak_and_keeps_live_children() {
    let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
    let _no_gc = crate::gc::GcSuppressScope::new();
    reset_for_test();
    unsafe {
        let proof = SharedLayout::shape_cache_entry();
        let empty = empty_backing(&proof);
        assert_eq!(empty, empty_backing(&proof));
        assert!(!empty.as_ptr().is_null());
        assert_eq!(empty.len(), 0);
        assert_eq!((*empty.as_ptr()).length, 0);
        assert!(crate::arena::pointer_in_old_gen(empty.addr()));
        let child = extend_key(&proof, empty, key("empty_root_child"));
        assert_ne!(empty.addr(), child.addr());
        prune_dead_canonical_keys(&|addr| addr == empty.addr());
        with_table_or((), |t| {
            assert!(!t.nodes[ROOT_NODE as usize].published);
            assert!(validated_node(t, empty.as_ptr(), 0, ROOT_NODE).is_none());
        });
        assert_eq!(
            extend_key(&proof, CanonicalKeys::EMPTY, key("empty_root_child")),
            child
        );
        let renewed = empty_backing(&proof);
        assert_eq!(renewed, empty_backing(&proof));
        assert_ne!(
            renewed.addr(),
            empty.addr(),
            "pruned backing is not retained"
        );
    }
}

#[test]
fn owned_list_is_published_without_copying_and_keeps_key_facts_after_gc() {
    let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    reset_for_test();
    unsafe {
        let proof = SharedLayout::shape_cache_entry();
        let scope = crate::gc::RuntimeHandleScope::new();
        let string = scope.root_string_ptr(key("owned-layout-key"));
        let symbol = JSValue::from_bits(crate::symbol::js_symbol_well_known_iterator().to_bits());
        let source = crate::object::key_attrs::alloc_canonical_key_list(2, true, true);
        let slots = crate::array::array_elements_ptr(source);
        let name = JSValue::string_ptr(string.get_raw_mut_ptr());
        *slots = name.bits();
        *slots.add(1) = symbol.bits();
        let attrs = crate::object::key_attrs::keys_attrs(source);
        crate::object::key_attrs::attrs_write(
            attrs,
            0,
            crate::object::key_attrs::ENTRY_NON_ENUMERABLE,
            name,
        );
        crate::object::key_attrs::attrs_write_slot(attrs, 0, 1);
        crate::object::key_attrs::attrs_write(
            attrs,
            1,
            crate::object::key_attrs::ENTRY_OPTIONAL,
            symbol,
        );
        crate::object::key_attrs::attrs_write_slot(attrs, 1, 0);
        (*source).length = 2;
        let expected = [
            crate::object::key_attrs::key_word(source, 0),
            crate::object::key_attrs::key_word(source, 1),
        ];
        let adopted = canonicalize_owned(&proof, source, 2);
        assert_eq!(adopted.as_ptr(), source, "a miss adopts its fresh backing");
        let root = scope.root_raw_mut_ptr(adopted.as_ptr());
        let borrowed = raw_list(&["owned-layout-key"]);
        let prefix = canonicalize(&proof, borrowed, 1);
        assert_ne!(
            prefix.as_ptr(),
            adopted.as_ptr(),
            "attributes and order are identity"
        );
        crate::gc::js_gc_collect();
        root.with_const_ptr(|keys: *const ArrayHeader| {
            assert_eq!(canonicalize(&proof, keys, 2).as_ptr(), adopted.as_ptr());
            for (i, word) in expected.iter().enumerate() {
                assert_eq!(crate::object::key_attrs::key_word(keys, i as u32), *word);
            }
            let (values, len) = crate::object::keys_array_dense_slots(keys);
            assert!(len >= 2);
            assert_eq!((*values.add(1)).to_bits(), symbol.bits());
        });
        let duplicate = crate::object::key_attrs::alloc_canonical_key_list(2, false, true);
        let src = root.get_raw_mut_ptr::<ArrayHeader>();
        // GC_STORE_AUDIT(INIT): Duplicate is a fresh unpublished stationary old canonical-key list. Source already passed publish_canonical, which prepares old-parent keys (canonical_keys.rs1483-1500), and survives explicit collect. Copy of two existing stable canonical key values plus attributes runs allocation-free; canonicalize_owned then returns existing identity.
        std::ptr::copy_nonoverlapping(
            crate::array::array_elements_ptr(src),
            crate::array::array_elements_ptr(duplicate),
            2,
        );
        crate::object::key_attrs::copy_entries(
            src,
            0,
            crate::object::key_attrs::keys_attrs(duplicate),
            2,
        );
        (*duplicate).length = 2;
        assert_eq!(
            canonicalize_owned(&proof, duplicate, 2),
            adopted,
            "a hit keeps one identity"
        );
    }
}

// ---------------------------------------------------------------------------
// CKI S1: the trie node is the list's identity, carried by its holders.
// ---------------------------------------------------------------------------

fn slot_reads() -> u64 {
    SLOT_READS.with(|c| c.get())
}

/// A handle carries its node; a raw spelling of the same list walks to the
/// same node; a carried view is its own proof and reads no slot at all.
#[test]
fn a_carried_node_is_the_lists_identity_and_needs_no_walk() {
    let _lock = crate::gc::global_side_table_test_lock();
    reset_for_test();
    unsafe {
        let proof = SharedLayout::shape_cache_entry();
        let mut grown = CanonicalKeys::EMPTY;
        for name in ["cki_a", "cki_b", "cki_c"] {
            grown = extend_key(&proof, grown, key(name));
        }
        assert_ne!(grown.node(), NO_NODE);
        assert!(node_publishes_for_test(grown.view()));

        let raw = raw_list(&["cki_a", "cki_b", "cki_c"]);
        let walked = canonicalize(&proof, raw, 3);
        assert_eq!(walked, grown);
        assert_eq!(walked.node(), grown.node(), "one list, one node");

        let before = slot_reads();
        let carried = canonicalize_keys(&proof, grown.view());
        assert_eq!(slot_reads(), before, "a carried node reads no slot");
        assert_eq!((carried, carried.node()), (grown, grown.node()));

        // A raw spelling of a canonical array (no node) still reaches it.
        let unknown = crate::object::ObjectKeys::new(grown.as_ptr(), 3);
        let found = canonicalize_keys(&proof, unknown);
        assert_eq!((found, found.node()), (grown, grown.node()));
        assert!(slot_reads() > before, "a node-less view walks the edges");
    }
}

/// The prefix of a carried list is its ancestor: the parent links answer
/// with no hash and no slot read, and agree with the content walk.
#[test]
fn a_prefix_of_a_carried_list_is_its_ancestor_node() {
    let _lock = crate::gc::global_side_table_test_lock();
    reset_for_test();
    unsafe {
        let proof = SharedLayout::shape_cache_entry();
        let one = extend_key(&proof, CanonicalKeys::EMPTY, key("cki_p1"));
        let two = extend_key(&proof, one, key("cki_p2"));
        let three = extend_key(&proof, two, key("cki_p3"));
        assert_eq!(three.as_ptr(), one.as_ptr(), "premise: one shared backing");
        let before = slot_reads();
        let prefix = canonical_prefix(&proof, three.view(), 2);
        assert_eq!(slot_reads(), before);
        assert_eq!((prefix, prefix.node()), (two, two.node()));
        let raw = raw_list(&["cki_p1", "cki_p2", "cki_p3"]);
        let walked = canonicalize(&proof, raw, 2);
        assert_eq!((walked, walked.node()), (two, two.node()));
    }
}

/// A holder whose node was freed and REUSED for another list gets `None`
/// from the trie, never the other list: the carried node is validated
/// against the holder's own `(backing, count)` on every use.
#[test]
fn a_freed_and_reused_node_never_answers_for_its_old_list() {
    let _lock = crate::gc::global_side_table_test_lock();
    let mut table = CanonicalTable::new();
    let old = table.alloc_node(0x1000, ROOT_NODE, 11, 1, true, true);
    assert_eq!(
        validated_node(&table, 0x1000 as *const ArrayHeader, 1, old),
        Some(old)
    );
    table.free_nodes(&[old]);
    assert_eq!(
        validated_node(&table, 0x1000 as *const ArrayHeader, 1, old),
        None
    );
    let reused = table.alloc_node(0x2000, ROOT_NODE, 12, 1, true, true);
    assert_eq!(reused, old, "premise: the free list hands the id back");
    assert_eq!(
        validated_node(&table, 0x1000 as *const ArrayHeader, 1, old),
        None
    );
    assert_eq!(
        validated_node(&table, 0x2000 as *const ArrayHeader, 2, reused),
        None
    );
    assert_eq!(
        validated_node(&table, 0x2000 as *const ArrayHeader, 1, reused),
        Some(reused)
    );
    // An unpublished witness does not answer either (rewitness).
    table.rewitness(reused, 0x3000);
    assert_eq!(
        validated_node(&table, 0x2000 as *const ArrayHeader, 1, reused),
        None
    );
    assert_eq!(
        validated_node(&table, 0x3000 as *const ArrayHeader, 1, reused),
        None
    );
    table.free_nodes(&[reused]);
}

/// A symbol slot's edge hash is its `SymbolHeader.id`, never its address:
/// the same symbol header at two addresses hashes alike, so the trie's
/// occupancy cannot move with the layout. The sabotage this exists for is
/// hashing `v.bits()` again, which makes the two hashes differ.
#[test]
fn a_symbol_edge_hashes_its_serial_not_its_address() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let sym = crate::symbol::persistent_symbol("cki-edge");
        // The same header (magic, description, serial) at another address,
        // as a moving collection or another link layout would place it.
        struct Moved {
            symbol: crate::symbol::SymbolHeader,
        }
        let moved = Box::leak(Box::new(Moved {
            symbol: std::ptr::read(sym),
        }));
        let at = |p: *const crate::symbol::SymbolHeader| {
            JSValue::from_bits(crate::value::POINTER_TAG | p as u64)
        };
        let here = Appended::Slot(at(sym)).slot_hash();
        let there = Appended::Slot(at(&moved.symbol)).slot_hash();
        assert_ne!(sym as usize, &moved.symbol as *const _ as usize);
        assert_eq!(
            here, there,
            "a symbol's edge must not depend on where it lives"
        );
        // A hole keeps its constant word's hash.
        let hole = JSValue::from_bits(crate::value::TAG_HOLE);
        assert_eq!(
            Appended::Slot(hole).slot_hash(),
            crate::value::TAG_HOLE ^ 0x9E37_79B9_7F4A_7C15
        );
    }
}

/// Symbol keys are edges: two symbols with ONE description are two lists
/// (the exact slot check, not the hash, decides), and one symbol appended
/// twice reaches its node again.
#[test]
fn symbols_with_one_description_are_distinct_edges() {
    let _lock = crate::gc::global_side_table_test_lock();
    reset_for_test();
    unsafe {
        let proof = SharedLayout::shape_cache_entry();
        let s1 = crate::symbol::persistent_symbol("cki-same");
        let s2 = crate::symbol::persistent_symbol("cki-same");
        assert_ne!(s1, s2);
        assert_ne!((*s1).id, (*s2).id);
        let slot = |s: *mut crate::symbol::SymbolHeader| {
            Appended::Slot(JSValue::from_bits(crate::value::POINTER_TAG | s as u64))
        };
        let base = extend_key(&proof, CanonicalKeys::EMPTY, key("cki_sym_base"));
        let a = extend_slot(&proof, base, slot(s1), 0);
        let b = extend_slot(&proof, base, slot(s2), 0);
        assert_ne!((a.as_ptr(), a.node()), (b.as_ptr(), b.node()));
        let again = extend_slot(&proof, base, slot(s1), 0);
        assert_eq!((again, again.node()), (a, a.node()));
    }
}

/// The shape is the authority: a record minted from a canonical handle
/// carries the list's node, and a receiver's keys view hands it back. A
/// record minted from a node-less spelling learns the node from the first
/// holder that brings one; it never enters identity (one record either way).
#[test]
fn the_shape_record_carries_its_lists_node() {
    let _lock = crate::gc::global_side_table_test_lock();
    reset_for_test();
    unsafe {
        let proof = SharedLayout::shape_cache_entry();
        let mut list = CanonicalKeys::EMPTY;
        for name in ["cki_rec_a", "cki_rec_b"] {
            list = extend_key(&proof, list, key(name));
        }
        let word = crate::object::shapes::shape_descriptor_ensure(list, 2, 2).unwrap();
        let record = crate::object::shapes::shape_record_for_word(word).unwrap();
        assert_eq!(record.keys_view().node(), list.node());
        assert!(node_publishes_for_test(record.keys_view()));
        // The packed element kind is untouched by the node.
        assert_eq!(record.element_kind(), 0);

        let mut other = CanonicalKeys::EMPTY;
        for name in ["cki_rec_c", "cki_rec_d", "cki_rec_e"] {
            other = extend_key(&proof, other, key(name));
        }
        // CKI S2: a node-less spelling of a published list resolves the
        // list's node at mint, so the record carries it from birth.
        let raw_word =
            crate::object::shapes::shape_descriptor_ensure(other.as_const_ptr(), 3, 3).unwrap();
        let raw_record = crate::object::shapes::shape_record_for_word(raw_word).unwrap();
        assert_eq!(raw_record.keys_view().node(), other.node());
        let carried = crate::object::shapes::shape_descriptor_ensure(other, 3, 3).unwrap();
        assert_eq!(carried, raw_word, "one list, one shape, whichever holder");
        // A node recorded for one count never answers for another: the
        // 2-key prefix on the same backing is its own (published) list.
        let short = crate::object::shapes::shape_descriptor_ensure(other, 2, 2).unwrap();
        let short_record = crate::object::shapes::shape_record_for_word(short).unwrap();
        assert_ne!(short_record.keys_view().node(), other.node());
        assert!(node_publishes_for_test(short_record.keys_view()));
    }
}
