//! CKI S2: a canonical list's shape state (its family, its backing's slot
//! index) lives on its trie node, and the exact-facts fold names the list by
//! that node, so nothing the shape table hashes depends on where the link put
//! the image.

use super::*;
use crate::object::canonical_keys::{self, CanonicalKeys, SharedLayout};

fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

unsafe fn list(names: &[&str]) -> CanonicalKeys {
    let proof = SharedLayout::shape_cache_entry();
    let mut keys = CanonicalKeys::EMPTY;
    for name in names {
        keys = canonical_keys::extend_key(&proof, keys, key(name));
    }
    keys
}

fn fresh_tables() {
    canonical_keys::reset_for_test();
    test_clear_shape_table();
}

fn on_node_family(node: u32, id: LayoutWord) -> bool {
    canonical_keys::with_list_shapes(node, |list| list.family.contains(id)).unwrap_or(false)
}

fn bucket_key(id: LayoutWord) -> u64 {
    let record = crate::state::state()
        .shapes
        .slab()
        .get_handle(id)
        .expect("live record");
    record.facts_key_with_keys(record.keys)
}

/// One list, one family, whichever holder asks: a handle carrying the node and
/// a raw spelling of the same array mint the same shape, filed on the node and
/// not in the owned (address-keyed) map.
#[test]
fn a_canonical_lists_family_lives_on_its_node_whatever_the_holder() {
    let _lock = crate::gc::global_side_table_test_lock();
    fresh_tables();
    unsafe {
        let keys = list(&["cki_s2_a", "cki_s2_b"]);
        let carried = shape_descriptor_ensure(keys, 2, 2).unwrap();
        let raw = shape_descriptor_ensure(keys.as_ptr(), 2, 2).unwrap();
        assert_eq!(carried, raw, "a node-less spelling resolves the same list");
        let record = crate::state::state().shapes.slab().get_handle(carried).unwrap();
        assert_eq!(record.keys_node(), keys.node());
        assert!(on_node_family(keys.node(), carried));
        let inner = crate::state::state().shapes.inner.borrow();
        assert!(inner.owned_families.get(&(keys.as_ptr() as u64)).is_none());
    }
}

/// An owned array has no node: its family is keyed by its heap address.
#[test]
fn an_owned_lists_family_stays_keyed_by_its_heap_address() {
    let _lock = crate::gc::global_side_table_test_lock();
    fresh_tables();
    let scope = crate::gc::RuntimeHandleScope::new();
    let owned = scope.root_raw_mut_ptr(crate::array::js_array_alloc(4));
    let arr = owned.get_raw_mut_ptr::<ArrayHeader>();
    let name = crate::value::js_nanbox_string(key("cki_s2_own") as i64);
    let arr = crate::array::js_array_push_f64(arr, name);
    owned.set_raw_mut_ptr(arr);
    let id = shape_descriptor_ensure(arr, 1, 1).unwrap();
    let record = crate::state::state().shapes.slab().get_handle(id).unwrap();
    assert_eq!(record.keys_node(), canonical_keys::NO_NODE);
    let inner = crate::state::state().shapes.inner.borrow();
    assert!(inner
        .owned_families
        .get(&(arr as u64))
        .is_some_and(|ids| ids.contains(id)));
}

/// The determinism test: the same program state built twice, once with the
/// list's backing at another address, folds every shape into the same
/// accelerator bucket. Putting the keys address back into the fold (the S2
/// sabotage) makes the two runs disagree.
#[test]
fn the_facts_fold_does_not_depend_on_the_lists_address() {
    let _lock = crate::gc::global_side_table_test_lock();
    let names = ["cki_s2_d1", "cki_s2_d2", "cki_s2_d3"];
    let mut runs = Vec::new();
    for shift in [0usize, 7] {
        fresh_tables();
        unsafe {
            let scope = crate::gc::RuntimeHandleScope::new();
            // Move every later allocation: the backings of run 2 land
            // elsewhere, as an image moved by the link would.
            let _pad: Vec<_> = (0..shift)
                .map(|_| scope.root_raw_mut_ptr(crate::array::js_array_alloc(64)))
                .collect();
            let _old_pad = crate::object::key_attrs::alloc_canonical_key_list(32 * shift as u32 + 1, true, false);
            let keys = list(&names);
            let id = shape_descriptor_ensure(keys, 3, 3).unwrap();
            runs.push((keys.as_ptr() as usize, keys.node(), bucket_key(id)));
        }
    }
    assert_ne!(runs[0].0, runs[1].0, "premise: the backing moved");
    assert_eq!(runs[0].1, runs[1].1, "node ids follow program order");
    assert_eq!(runs[0].2, runs[1].2, "the bucket names the list, not its address");
}

/// A prefix and its tip append share one backing; the slot index lives on
/// the node that owns that backing, so both lists answer from one index and
/// the owned map holds nothing for it.
#[test]
fn a_backings_slot_index_lives_on_its_owner_node() {
    let _lock = crate::gc::global_side_table_test_lock();
    fresh_tables();
    unsafe {
        let names: Vec<String> = (0..40).map(|i| format!("cki_s2_w{i}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let short = list(&names[..36]);
        let long = list(&names);
        assert_eq!(short.as_ptr(), long.as_ptr(), "premise: one shared backing");
        let probe = names[30].as_bytes();
        let hash = crate::object::key_bytes_hash(probe.as_ptr(), probe.len());
        let found = shape_slot_lookup(short.view(), probe, hash, 36, true);
        assert_eq!(found, Some(30));
        let found = shape_slot_lookup(long.view(), probe, hash, 40, true);
        assert_eq!(found, Some(30));
        assert_eq!(test_shape_index_len(long.as_ptr() as usize), 40);
        let mut homes = 0;
        canonical_keys::for_each_list_shapes(|_, addr, list| {
            if addr == long.as_ptr() as usize && list.index.is_some() {
                homes += 1;
            }
        });
        assert_eq!(homes, 1, "one index per backing");
        assert!(crate::state::state().shapes.inner.borrow().owned_indices.is_empty());
        // A complete index proves absence.
        let missing = b"cki_s2_missing";
        let hash = crate::object::key_bytes_hash(missing.as_ptr(), missing.len());
        assert!(matches!(
            shape_slot_lookup_verdict(long.view(), missing, hash, 40, true),
            KeysIndexVerdict::Absent
        ));
    }
}

/// A dead backing's descriptors leave its node's family in the shape prune,
/// before the trie frees the node, so a reused node starts empty.
#[test]
fn a_dead_lists_family_retires_before_its_node_is_freed() {
    let _lock = crate::gc::global_side_table_test_lock();
    fresh_tables();
    unsafe {
        let keys = list(&["cki_s2_dead_a", "cki_s2_dead_b"]);
        let id = shape_descriptor_ensure(keys, 2, 2).unwrap();
        let node = keys.node();
        assert!(on_node_family(node, id));
        let dead = keys.as_ptr() as usize;
        prune_dead_shape_keys(&|addr| addr == dead);
        assert!(!on_node_family(node, id));
        assert!(shape_descriptor_for_word(id).is_none());
        canonical_keys::prune_dead_canonical_keys(&|addr| addr == dead);
        let reused = list(&["cki_s2_dead_c"]);
        assert!(canonical_keys::with_list_shapes(reused.node(), |l| l.family.is_empty())
            .unwrap_or(true));
    }
}

/// Q1's invariant: a node outlives every live holder of its array, across
/// moving collections; and once the array dies, the shape prune retires every
/// record on the list BEFORE the trie frees (or reuses) its node. This is what
/// lets the mint trust a carried node without validating it.
#[test]
fn a_node_outlives_its_holders_and_dies_after_its_records() {
    // The guard holds the nursery isolation lock the side-table lock takes.
    let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    fresh_tables();
    unsafe {
        // A live receiver in the nursery: its shape's carried node must keep
        // publishing its list while minors move the receiver and fulls run.
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        for name in ["cki_s2_live_a", "cki_s2_live_b", "cki_s2_live_c"] {
            let k = key(name);
            obj.with_mut_ptr(|o| crate::object::js_object_set_field_by_name(o, k, 1.0));
        }
        let before = obj.with_const_ptr::<crate::ObjectHeader, _>(|o| o as usize);
        for round in 0..3 {
            let _ = crate::gc::gc_collect_minor();
            if round == 1 {
                crate::gc::js_gc_collect();
            }
            let view = obj.with_const_ptr(|o| crate::object::object_keys(o));
            assert_ne!(view.node(), canonical_keys::NO_NODE, "premise: a canonical list");
            assert!(
                canonical_keys::node_publishes(view.arr(), view.count(), view.node()),
                "a live holder's node still publishes its list (round {round})"
            );
        }
        let after = obj.with_const_ptr::<crate::ObjectHeader, _>(|o| o as usize);
        assert_ne!(before, after, "premise: the receiver moved");

        // A list held only by a handle: once the handle goes, a full trace
        // retires its records, then frees its node.
        let (dead, node, id) = {
            let held = crate::gc::RuntimeHandleScope::new();
            let keys = list(&["cki_s2_dying_a", "cki_s2_dying_b"]);
            let _root = held.root_raw_mut_ptr(keys.as_ptr());
            let id = shape_descriptor_ensure(keys, 2, 2).unwrap();
            crate::gc::js_gc_collect();
            // An uncarried record may retire at any full trace; the list
            // and its node stay as long as the handle holds the array.
            assert!(canonical_keys::node_publishes(keys.as_ptr(), 2, keys.node()));
            (keys.as_ptr() as usize, keys.node(), id)
        };
        crate::gc::js_gc_collect();
        assert!(shape_descriptor_for_word(id).is_none(), "the record retired with its list");
        let mut stale = 0;
        crate::state::state().shapes.slab().for_each_handle(|_, record| {
            let record = &*record;
            if record.keys as usize == dead || (record.keys_node() == node && record.keys != 0) {
                stale += 1;
            }
        });
        assert_eq!(stale, 0, "no record names the dead list or its freed node");
        assert!(!canonical_keys::node_publishes(dead as *const ArrayHeader, 2, node));
    }
}
