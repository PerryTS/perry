//! #9754 — the side-table young-entry logs (`gc/young_log.rs`).
//!
//! Each table gets the same three-part proof:
//!
//! * a YOUNG entry reachable only through the table is moved by a copying
//!   minor and the entry re-keyed — through the minor-scoped walk, which the
//!   recorded walk row proves was PARTIAL (visited only the logged keys);
//! * an OLD entry is not visited at all — the row proves the skip fired
//!   (`visited == 0` while the table is non-empty), which is rule 3 of the
//!   design note: a latch that never skips looks landed while doing nothing;
//! * a DEAD young owner is pruned by the young-only prune.
//!
//! Sabotage contract (rule 2): delete any one `note` call in the tables'
//! writers and the matching "moves" test here goes red — under
//! `debug_assertions` on the log-completeness assertion the walk runs first,
//! and in release on the stale address the un-visited entry keeps.

use super::super::*;
use super::support::*;

fn young_closure() -> usize {
    let ptr = crate::arena::arena_alloc_gc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        std::mem::align_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe { init_test_closure(ptr) };
    ptr as usize
}

fn old_closure() -> usize {
    let ptr = crate::arena::arena_alloc_gc_old(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        std::mem::align_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe { init_test_closure(ptr) };
    ptr as usize
}

unsafe fn young_keys_array() -> *mut crate::array::ArrayHeader {
    let arr = crate::arena::arena_alloc_gc(
        std::mem::size_of::<crate::array::ArrayHeader>(),
        std::mem::align_of::<crate::array::ArrayHeader>(),
        GC_TYPE_ARRAY,
    ) as *mut crate::array::ArrayHeader;
    (*arr).length = 0;
    (*arr).capacity = 0;
    arr
}

fn walk(table: &'static str) -> young_log::YoungLogWalk {
    young_log::last_walk(table).unwrap_or_else(|| panic!("no walk recorded for {table}"))
}

// ---------------------------------------------------------------- closures

#[test]
fn young_closure_prop_value_is_traced_and_moved_by_a_minor() {
    let _guard = CopyingNurseryTestGuard::new(1);
    gc_register_mutable_root_scanner(crate::closure::scan_closure_dynamic_props_roots_mut);

    let owner = young_closure();
    js_shadow_slot_set(0, ptr_bits(owner));
    // The value is reachable ONLY through the side table.
    let value = young_leaf();
    crate::closure::closure_set_dynamic_prop(owner, "memo", f64::from_bits(string_bits(value)));

    let _ = gc_collect_minor();

    let owner_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(
        owner_after, owner,
        "the rooted owner must have been evacuated"
    );
    let bits = crate::closure::closure_get_own_dynamic_prop(owner_after, "memo")
        .expect("entry must follow its owner to the new address")
        .to_bits();
    let value_after = (bits & POINTER_MASK) as usize;
    assert_eq!(bits & TAG_MASK, STRING_TAG);
    assert_ne!(
        value_after, value,
        "the value must have been evacuated, not left in from-space"
    );
    assert!(crate::arena::pointer_in_nursery(value_after));
    assert!(
        crate::closure::closure_get_own_dynamic_prop(owner, "memo").is_none(),
        "the stale owner key must be gone"
    );
    // #9841 re-pointed this at the CURRENT contract: `closure.dynamic_props`
    // takes the FULL walk on a copying minor, because its young walk measured
    // worse than full on 223/273 minor cycles. The rooting property above is
    // what this test is really for and is unchanged — the value reachable
    // only through the side table is still traced and evacuated. What changed
    // is which walk does it.
    let row = walk("closure.dynamic_props");
    assert!(
        !row.partial,
        "closure.dynamic_props takes the FULL walk since #9841: {row:?}"
    );
    assert_eq!(
        row.visited, row.table_len,
        "a full walk visits every owner: {row:?}"
    );
    assert!(
        row.kept >= 1,
        "the full walk must REBUILD the log for the death prune, which is now \
         its only consumer: {row:?}"
    );
}

#[test]
fn young_value_under_an_old_closure_owner_is_traced_by_a_minor() {
    let _guard = CopyingNurseryTestGuard::new(0);
    gc_register_mutable_root_scanner(crate::closure::scan_closure_dynamic_props_roots_mut);

    let owner = old_closure();
    let value = young_leaf();
    crate::closure::closure_set_dynamic_prop(owner, "memo", f64::from_bits(string_bits(value)));
    let proto = young_leaf();
    crate::closure::closure_set_static_prototype(owner, string_bits(proto));

    let _ = gc_collect_minor();

    let bits = crate::closure::closure_get_own_dynamic_prop(owner, "memo")
        .expect("old owner keeps its entry")
        .to_bits();
    let value_after = (bits & POINTER_MASK) as usize;
    assert_ne!(value_after, value);
    assert!(crate::arena::pointer_in_nursery(value_after));
    let proto_after = (crate::closure::closure_static_prototype(owner).expect("prototype kept")
        & POINTER_MASK) as usize;
    assert_ne!(proto_after, proto);
    assert!(crate::arena::pointer_in_nursery(proto_after));
    // The value/prototype rooting above is the property under test and holds
    // unchanged. The walk is now the full one (#9841); the log this test was
    // named for is still ARMED by the value (`note_young_closure_owner`), but
    // its only consumer is now the death prune, which keys on OWNERS, so
    // value-based arming has no reader left. Flagged rather than removed
    // here: dropping it is a separate change with its own measurement.
    let row = walk("closure.dynamic_props");
    assert!(
        !row.partial,
        "closure.dynamic_props takes the FULL walk since #9841: {row:?}"
    );
}

#[test]
fn old_closure_entries_survive_a_minor_full_walk() {
    let _guard = CopyingNurseryTestGuard::new(0);
    gc_register_mutable_root_scanner(crate::closure::scan_closure_dynamic_props_roots_mut);

    let owner = old_closure();
    crate::closure::closure_set_dynamic_prop(owner, "count", 42.0);
    crate::closure::closure_mark_key_deleted(owner, "name");

    let _ = gc_collect_minor();

    assert_eq!(
        crate::closure::closure_get_own_dynamic_prop(owner, "count"),
        Some(42.0)
    );
    assert!(crate::closure::closure_is_key_deleted(owner, "name"));
    // Was: "must not be visited by a minor", asserting the young walk's skip.
    // #9841 withdrew that skip for this table — it was measured at 2.2-2.7 %
    // and worse than full on the great majority of minor cycles — so the
    // contract is now the opposite and is asserted as such. The surviving
    // property is the one above: an old owner's entries are intact after a
    // minor. There is no skip left to observe for this table, which is
    // exactly the finding.
    let row = walk("closure.dynamic_props");
    assert!(
        !row.partial,
        "closure.dynamic_props takes the FULL walk since #9841: {row:?}"
    );
    assert!(row.table_len >= 2, "{row:?}");
    assert_eq!(
        row.visited, row.table_len,
        "a full walk visits every owner, old ones included: {row:?}"
    );
}

#[test]
fn dead_young_closure_owner_is_pruned_by_the_young_prune() {
    let _guard = CopyingNurseryTestGuard::new(1);
    gc_register_mutable_root_scanner(crate::closure::scan_closure_dynamic_props_roots_mut);
    // One rooted young object so the minor has real work; the owner is not it.
    js_shadow_slot_set(0, string_bits(young_leaf()));

    let dead = young_closure();
    crate::closure::closure_set_dynamic_prop(dead, "memo", 42.0);
    crate::closure::closure_set_static_prototype(dead, crate::value::TAG_NULL);
    assert!(crate::closure::closure_get_own_dynamic_prop(dead, "memo").is_some());

    let _ = gc_collect_minor();

    assert!(
        crate::closure::closure_get_own_dynamic_prop(dead, "memo").is_none(),
        "the dead young owner's CLOSURE_PROPS entry must be pruned from the log"
    );
    assert!(crate::closure::closure_static_prototype(dead).is_none());
}

// -------------------------------------------------------------- descriptors

#[test]
fn young_accessor_getter_is_moved_through_the_log() {
    let _guard = CopyingNurseryTestGuard::new(1);
    gc_register_mutable_root_scanner(crate::object::descriptor_state::scan_descriptor_roots_mut);

    let (owner, _) = unsafe { alloc_nursery_test_object(0) };
    let owner = owner as usize;
    js_shadow_slot_set(0, ptr_bits(owner));
    // The getter closure is reachable ONLY through the accessor table.
    let getter = young_closure();
    crate::object::set_accessor_descriptor(
        owner,
        "g".to_string(),
        crate::object::AccessorDescriptor {
            get: ptr_bits(getter),
            set: 0,
        },
    );

    let _ = gc_collect_minor();

    let owner_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(owner_after, owner);
    let acc = crate::object::get_accessor_descriptor(owner_after, "g")
        .expect("accessor must follow its owner to the new address");
    let getter_after = (acc.get & POINTER_MASK) as usize;
    assert_ne!(getter_after, getter, "the getter must have been evacuated");
    assert!(crate::arena::pointer_in_nursery(getter_after));
    assert!(crate::object::get_accessor_descriptor(owner, "g").is_none());
    let row = walk("object.descriptors");
    assert!(row.partial);
    assert!(row.visited >= 1, "{row:?}");
}

#[test]
fn old_descriptor_owners_are_skipped_by_a_minor() {
    let _guard = CopyingNurseryTestGuard::new(0);
    gc_register_mutable_root_scanner(crate::object::descriptor_state::scan_descriptor_roots_mut);

    // The descriptor tables are agent state that outlives every test on this
    // thread, and the FIRST descriptor install on a thread bootstraps the
    // lazy `globalThis` realm (#7975), which installs ~1.8k builtin
    // descriptors on young objects. Warm that up, take a minor, then measure
    // the delta: the old entry must add index rows but no visit.
    let (warm, _) = unsafe { alloc_old_test_object(0) };
    crate::object::set_property_attrs(
        warm as usize,
        "warm".to_string(),
        crate::object::PropertyAttrs::new(false, true, true),
    );
    let _ = gc_collect_minor();
    let before = walk("object.descriptors");

    let (owner, _) = unsafe { alloc_old_test_object(0) };
    let owner = owner as usize;
    let getter = old_closure();
    crate::object::set_accessor_descriptor(
        owner,
        "g".to_string(),
        crate::object::AccessorDescriptor {
            get: ptr_bits(getter),
            set: 0,
        },
    );
    crate::object::set_property_attrs(
        owner,
        "p".to_string(),
        crate::object::PropertyAttrs::new(false, true, true),
    );

    let _ = gc_collect_minor();

    assert_eq!(
        crate::object::get_accessor_descriptor(owner, "g").map(|acc| acc.get),
        Some(ptr_bits(getter))
    );
    let row = walk("object.descriptors");
    assert!(row.partial);
    // The first minor's prune can drop dead realm owners between the two
    // walks, so the exact count is `kept` minus whatever died; the new old
    // entry can only NOT add to it.
    assert!(
        row.visited <= before.kept,
        "old owner, old getter: the new entry must not add a visit: {before:?} -> {row:?}"
    );
    assert!(
        row.visited < row.table_len,
        "the walk must stay partial: {row:?}"
    );
}

#[test]
fn dead_young_descriptor_owner_is_pruned_by_the_young_prune() {
    let _guard = CopyingNurseryTestGuard::new(1);
    gc_register_mutable_root_scanner(crate::object::descriptor_state::scan_descriptor_roots_mut);
    js_shadow_slot_set(0, string_bits(young_leaf()));

    let (dead, _) = unsafe { alloc_nursery_test_object(0) };
    let dead = dead as usize;
    crate::object::set_property_attrs(
        dead,
        "p".to_string(),
        crate::object::PropertyAttrs::new(false, true, true),
    );
    assert!(crate::object::get_property_attrs(dead, "p").is_some());

    let _ = gc_collect_minor();

    assert!(
        crate::object::get_property_attrs(dead, "p").is_none(),
        "the dead young owner's descriptor must be pruned from the log"
    );
}

// ------------------------------------------------------------------- shapes

#[test]
fn young_keys_array_family_is_rekeyed_through_the_log() {
    let _guard = CopyingNurseryTestGuard::new(1);
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);

    let keys = unsafe { young_keys_array() };
    js_shadow_slot_set(0, ptr_bits(keys as usize));
    let id = crate::object::shapes::shape_descriptor_ensure(keys, 0, 0).expect("shape id");
    assert_eq!(
        crate::object::shapes::shape_descriptor_by_id(id).map(|d| d.keys),
        Some(keys as u64)
    );

    let _ = gc_collect_minor();

    let keys_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(
        keys_after, keys as usize,
        "the rooted keys array must have moved"
    );
    assert_eq!(
        crate::object::shapes::shape_descriptor_by_id(id).map(|d| d.keys),
        Some(keys_after as u64),
        "the family's descriptor must be re-keyed to the evacuated keys array"
    );
    let row = walk("shapes.families+indices");
    assert!(row.partial);
    assert!(row.visited >= 1, "{row:?}");
}

#[test]
fn old_shape_families_are_skipped_by_a_minor() {
    let _guard = CopyingNurseryTestGuard::new(0);
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);

    let keys = crate::arena::arena_alloc_gc_old(
        std::mem::size_of::<crate::array::ArrayHeader>(),
        std::mem::align_of::<crate::array::ArrayHeader>(),
        GC_TYPE_ARRAY,
    ) as *mut crate::array::ArrayHeader;
    unsafe {
        (*keys).length = 0;
        (*keys).capacity = 0;
    }
    let id = crate::object::shapes::shape_descriptor_ensure(keys, 0, 0).expect("shape id");

    let _ = gc_collect_minor();

    assert_eq!(
        crate::object::shapes::shape_descriptor_by_id(id).map(|d| d.keys),
        Some(keys as u64)
    );
    let row = walk("shapes.families+indices");
    assert!(row.partial);
    assert!(row.table_len >= 1, "{row:?}");
    assert_eq!(
        row.visited, 0,
        "an old keys array's family must not be visited: {row:?}"
    );
}

// ------------------------------------------------------------------- caches

#[test]
fn young_transition_cache_target_is_rewritten_through_the_log() {
    let _guard = CopyingNurseryTestGuard::new(1);
    gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);

    let keys = unsafe { young_keys_array() } as usize;
    js_shadow_slot_set(0, ptr_bits(keys));
    // A predecessor that resolves, or the copied-minor prune retires the entry
    // (`shape_descriptor_by_id(0)` is `None`) before the assertion reads it.
    let prev =
        crate::object::shapes::shape_descriptor_ensure(std::ptr::null(), 0, 1).expect("shape id");
    crate::object::test_seed_transition_cache_root_for_shape(prev, keys);

    let _ = gc_collect_minor();

    let keys_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(keys_after, keys);
    assert_eq!(
        crate::object::test_transition_cache_root(),
        keys_after,
        "the cached target must be rewritten to the evacuated keys array"
    );
    let row = walk("object.transition_cache");
    assert!(row.partial);
    assert!(row.visited >= 1, "{row:?}");
    assert!(
        row.visited < row.table_len,
        "a 16k-slot table must not be walked whole: {row:?}"
    );
}

/// The PRODUCTION transition-cache writer arms on EITHER address, and the
/// second clause — a young interned KEY under an old target — was covered by
/// no test: all three `#[cfg(test)]` seeds either ignored the key or
/// classified it unconditionally, so `transition_cache_insert`'s own
/// `(len_marker == 0 && addr_is_minor_relevant(kid))` arm could be deleted
/// while the suite stayed green. This drives the real writer.
#[test]
fn young_transition_key_under_an_old_target_arms_the_log_through_the_writer() {
    let _guard = CopyingNurseryTestGuard::new(1);
    gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);
    crate::object::test_clear_transition_cache_root();

    // Target OLD: the first clause of the predicate is false for it.
    let old_target = unsafe {
        let arr = crate::arena::arena_alloc_gc_old(
            std::mem::size_of::<crate::array::ArrayHeader>(),
            std::mem::align_of::<crate::array::ArrayHeader>(),
            GC_TYPE_ARRAY,
        ) as *mut crate::array::ArrayHeader;
        (*arr).length = 0;
        (*arr).capacity = 0;
        arr as usize
    };
    assert!(!crate::arena::pointer_in_nursery(old_target));

    // Key YOUNG, and long enough that `transition_key_id` keeps it a POINTER
    // (`len_marker == 0`) rather than packing it as a length.
    let key = crate::string::js_string_from_bytes(b"young-transition-key".as_ptr(), 20);
    assert!(crate::arena::pointer_in_nursery(key as usize));

    let before = young_log::last_walk("object.transition_cache");
    crate::object::test_transition_cache_insert(0, key, old_target, 0, 0);

    // The log must now name the slot the writer published into. Reading it
    // through a minor is the same observation the scanner makes.
    let _ = gc_collect_minor();
    let row = walk("object.transition_cache");
    assert!(row.partial, "{row:?}");
    assert!(
        row.visited >= 1,
        "the young KEY must have armed the log: {row:?} (before: {before:?})"
    );
}

#[test]
fn young_shape_cache_entry_is_moved_through_the_log() {
    let _guard = CopyingNurseryTestGuard::new(0);
    gc_register_mutable_root_scanner(crate::object::scan_shape_cache_roots_mut);

    // Reachable ONLY through the cache (which roots it). Seeded through the
    // PRODUCTION writer (`shape_cache_insert`), not a test seam: a seam that
    // arms the log itself makes this test pass with the writer's own arm site
    // deleted, which is how #9755 shipped an unenforced rule 1.
    let keys = unsafe { young_keys_array() };
    let shape_id = 0x9754_0001;
    crate::object::test_shape_cache_insert(shape_id, keys);

    let _ = gc_collect_minor();

    let (inline, overflow) = crate::object::test_shape_cache_root(shape_id);
    assert_ne!(
        overflow, keys as usize,
        "the overflow entry must have been evacuated"
    );
    assert!(crate::arena::pointer_in_nursery(overflow));
    assert_eq!(
        inline, overflow,
        "inline and overflow must agree on the new address"
    );
    let row = walk("object.shape_cache");
    assert!(row.partial);
    assert!(row.visited >= 1, "{row:?}");
}

// ---------------------------------------------------------------------------
// `shapes.indices` arming (#9756 restructures this table; nothing exercised
// its four arm sites, so a missed `note` there was a collected live object
// that this suite would not have caught).
// ---------------------------------------------------------------------------

/// Keys count above `KEYS_INDEX_THRESHOLD` (32), so the index is built at all.
const INDEXED_KEYS: u32 = 40;

/// A YOUNG keys array of `INDEXED_KEYS` young string keys, in the dense
/// NaN-boxed layout `keys_array_dense_slots` reads.
unsafe fn young_indexed_keys_array() -> (*mut crate::array::ArrayHeader, Vec<Vec<u8>>) {
    let arr = crate::array::js_array_alloc_with_length(INDEXED_KEYS);
    let slots = (arr as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut f64;
    let mut names = Vec::new();
    for i in 0..INDEXED_KEYS {
        let name = format!("young_key_{i:04}");
        let s = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        *slots.add(i as usize) = f64::from_bits(string_bits(s as usize));
        names.push(name.into_bytes());
    }
    (*arr).length = INDEXED_KEYS;
    (arr, names)
}

unsafe fn build_index_for(keys: *mut crate::array::ArrayHeader, names: &[Vec<u8>]) {
    // `build = true` is the arm site: it inserts the `indices` entry.
    crate::object::shapes::test_build_slot_index(keys, &names[0], INDEXED_KEYS);
}

/// S17 — `shape_slot_lookup_verdict`'s build arm publishes an `indices` entry
/// keyed by a YOUNG keys address.
#[test]
fn building_a_slot_index_on_a_young_keys_array_arms_the_log() {
    let _guard = CopyingNurseryTestGuard::new(1);
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    crate::object::shapes::test_clear_shape_table();

    let (keys, names) = unsafe { young_indexed_keys_array() };
    js_shadow_slot_set(0, ptr_bits(keys as usize));
    assert!(crate::arena::pointer_in_nursery(keys as usize));
    unsafe { build_index_for(keys, &names) };
    assert!(crate::object::shapes::test_shape_index_len(keys as usize) > 0);

    // Rule 2 re-derives the relevant set from `indices` during the walk and
    // panics if the log does not name this address.
    let _ = gc_collect_minor();

    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(
        moved, keys as usize,
        "the keys array must have been evacuated"
    );
    assert!(
        crate::object::shapes::test_shape_index_len(moved) > 0,
        "the index must follow the keys array to its new address"
    );
}

/// S18 — `shape_keys_grown` re-keys the index onto the grown array's address.
#[test]
fn growing_an_indexed_keys_array_arms_the_log_for_the_new_address() {
    let _guard = CopyingNurseryTestGuard::new(1);
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    crate::object::shapes::test_clear_shape_table();

    let (old_keys, names) = unsafe { young_indexed_keys_array() };
    unsafe { build_index_for(old_keys, &names) };
    let (new_keys, _) = unsafe { young_indexed_keys_array() };
    js_shadow_slot_set(0, ptr_bits(new_keys as usize));

    crate::object::shapes::shape_keys_grown(old_keys as usize, new_keys);
    assert!(crate::object::shapes::test_shape_index_len(new_keys as usize) > 0);

    let _ = gc_collect_minor();
    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(moved, new_keys as usize);
    assert!(crate::object::shapes::test_shape_index_len(moved) > 0);
}

/// S19 — `shape_index_migrate_after_delete` moves a complete index onto the
/// compacted array's address.
#[test]
fn migrating_an_index_after_a_delete_arms_the_log_for_the_new_address() {
    let _guard = CopyingNurseryTestGuard::new(1);
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    crate::object::shapes::test_clear_shape_table();

    let (old_keys, names) = unsafe { young_indexed_keys_array() };
    unsafe { build_index_for(old_keys, &names) };
    let (new_keys, _) = unsafe { young_indexed_keys_array() };
    js_shadow_slot_set(0, ptr_bits(new_keys as usize));

    let migrated = crate::object::shapes::test_shape_index_migrate_after_delete(
        old_keys as usize,
        new_keys as usize,
        /* removed_slot = */ 0,
        INDEXED_KEYS,
        /* old_keys_shared = */ false,
    );
    assert!(migrated, "a complete index must migrate");

    let _ = gc_collect_minor();
    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(moved, new_keys as usize);
    assert!(crate::object::shapes::test_shape_index_len(moved) > 0);
}

/// S16 — `family_push_front`, the external-id install path, publishes a family
/// under a YOUNG keys address.
#[test]
fn installing_an_external_shape_id_arms_the_family_log() {
    let _guard = CopyingNurseryTestGuard::new(1);
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    crate::object::shapes::test_clear_shape_table();

    let keys = unsafe { young_keys_array() };
    js_shadow_slot_set(0, ptr_bits(keys as usize));
    let id = crate::object::shapes::test_unused_external_shape_id();
    assert!(
        crate::object::shapes::test_install_external_shape_id(id, keys, 0, 0),
        "the external id must install"
    );

    let _ = gc_collect_minor();
    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(moved, keys as usize);
    assert!(
        !crate::object::shapes::test_shape_ids_for_keys(moved).is_empty(),
        "the family must have followed the keys array"
    );
}

// -------------------------------------------------- per-object layout tables
//
// #9841: the DEATH PRUNE of `LAYOUT_SLOT_MASKS + TYPED_LAYOUTS`, not a root
// scanner. Its predicate is `layout_key_may_be_nursery`, which excludes
// `Longlived` AND `Old` — strictly stronger than the scanners'
// `addr_is_minor_relevant` — so an old-keyed record is not merely cheap to
// visit, it is provably impossible for a minor to remove.

use crate::gc::layout_tables::{test_per_object_layout_present, LAYOUT_YOUNG_LOG_NAME};

/// A nursery object whose header says POINTER_FREE and which then takes a
/// pointer store — the mutator path that mints a mask from inside
/// `layout_note_slot`'s own `borrow_mut` (WRITER 3). On cc that is the
/// dominant insert path: `TYPED_LAYOUTS` is empty there and every one of the
/// ~66k live keys is a `LAYOUT_SLOT_MASKS` entry.
fn young_masked_object() -> usize {
    let obj = crate::object::js_object_alloc(0, 8);
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::number(1.0));
    crate::object::js_object_set_field(obj, 1, crate::value::JSValue::number(2.0));
    crate::gc::layout_clear_for_ptr(obj as usize);
    unsafe { crate::gc::layout_init_pointer_free(obj as *mut u8) };
    let child = crate::string::js_string_from_bytes(b"late-pointer".as_ptr(), 12);
    crate::object::js_object_set_field(obj, 1, crate::value::JSValue::string_ptr(child));
    assert!(
        test_per_object_layout_present(obj as usize),
        "premise: the in-place mask mint published a per-object record"
    );
    obj as usize
}

/// WRITER 3's arming site. Delete `arm_young_layout_key` from
/// `gc/layout.rs`'s in-borrow mint and this goes red: under
/// `debug_assertions` on the log-completeness re-derivation, and in release
/// on the record the young prune can no longer see.
#[test]
fn dead_young_masked_owner_is_pruned_through_the_layout_log() {
    let _guard = CopyingNurseryTestGuard::new(1);
    // One rooted young object so the minor has real work; the owner is not it.
    js_shadow_slot_set(0, string_bits(young_leaf()));

    let dead = young_masked_object();

    let _ = gc_collect_minor();

    assert!(
        !test_per_object_layout_present(dead),
        "the dead young owner's per-object layout record must be pruned from the log"
    );
    let row = walk(LAYOUT_YOUNG_LOG_NAME);
    assert!(
        row.partial,
        "a copying minor must take the young-scoped prune: {row:?}"
    );
    assert!(
        row.visited >= 1,
        "the logged key must have been visited: {row:?}"
    );
}

/// WRITER 4's arming site (`transfer_per_object_slot_mask`, which runs during
/// evacuation and therefore BEFORE this collection's prune). Delete its
/// `arm_moved_layout_key` and the re-derivation panics here on the to-space
/// key.
#[test]
fn surviving_young_masked_owner_is_rekeyed_and_stays_logged() {
    let _guard = CopyingNurseryTestGuard::new(1);

    let obj = young_masked_object();
    js_shadow_slot_set(0, ptr_bits(obj));

    let _ = gc_collect_minor();

    let after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(after, obj, "the rooted owner must have been evacuated");
    assert!(
        test_per_object_layout_present(after),
        "the mask must follow its owner to the new address"
    );
    assert!(
        !test_per_object_layout_present(obj),
        "the stale from-space key must be gone"
    );
    let row = walk(LAYOUT_YOUNG_LOG_NAME);
    assert!(row.partial, "{row:?}");
    assert!(
        row.visited >= 1,
        "the move hook's key must have been logged and visited: {row:?}"
    );
    if crate::arena::pointer_in_nursery(after) {
        assert!(
            row.kept >= 1,
            "a survivor still in the nursery must stay logged: {row:?}"
        );
    }
}

/// Rule 3: the skip has to be observable, or a latch that never fires looks
/// landed. An OLD-keyed record cannot be found dead by any minor, so the
/// young prune must not visit it at all.
#[test]
fn old_layout_records_are_skipped_by_a_minor() {
    let _guard = CopyingNurseryTestGuard::new(0);

    // Drain whatever this thread's earlier tests left young, so `visited`
    // below is about the record installed after it.
    let _ = gc_collect_minor();

    let (owner, _) = unsafe { alloc_old_test_object(2) };
    crate::gc::layout_tables::slot_masks_insert(
        owner as usize,
        crate::gc::layout::LayoutSlotMask::from_words(&[1]),
    );

    let _ = gc_collect_minor();

    assert!(
        test_per_object_layout_present(owner as usize),
        "an old owner's record must survive a minor"
    );
    let row = walk(LAYOUT_YOUNG_LOG_NAME);
    assert!(row.partial, "{row:?}");
    assert!(row.table_len >= 1, "{row:?}");
    assert_eq!(
        row.visited, 0,
        "an old-keyed record is not a candidate for any minor and must not be \
         visited: {row:?}"
    );

    crate::gc::layout_clear_for_ptr(owner as usize);
}

// ------------------------------------------------- rule 2, proved able to fail
//
// #9841 moved `closure.dynamic_props`'s log-completeness re-derivation out of
// the minor-scoped SCANNER (withdrawn) and into
// `prune_dead_closure_side_table_owners_young`, which is now the log's only
// consumer. A guard that has been EXERCISED and never fired is not yet a
// guard: the campaign's own rule is that a check which cannot fail is
// documentation. This drives it to failure deliberately.
//
// It runs in a CHILD PROCESS because the expected outcome is a panic raised
// inside a collection. Depending on the frames between the prune and the test
// body that panic can cross an `extern "C"` boundary and abort the process
// ("panic in a function that cannot unwind"), which `#[should_panic]` cannot
// catch. Asserting on the child's exit status and stderr is robust to both
// outcomes, and is the same isolation pattern
// `test_armed_per_object_layout_thread_exit_disarms_global_count` uses.

#[cfg(debug_assertions)]
const RULE2_SABOTAGE_ENV: &str = "PERRY_TEST_CLOSURE_YOUNG_LOG_SABOTAGE_CHILD";
#[cfg(debug_assertions)]
const RULE2_SABOTAGE_TEST: &str =
    "gc::tests::young_log_tests::dropping_a_logged_closure_owner_trips_the_prune_rule2_check";

// Rule 2 is `#[cfg(debug_assertions)]`, so in a release test build there is no
// guard to trip and the child would exit 0. Gating the test the same way keeps
// it honest instead of red on the release rotation.
#[cfg(debug_assertions)]
#[test]
fn dropping_a_logged_closure_owner_trips_the_prune_rule2_check() {
    if std::env::var_os(RULE2_SABOTAGE_ENV).is_some() {
        let _guard = CopyingNurseryTestGuard::new(1);
        crate::closure::test_clear_closure_side_tables();
        // One rooted young object so the minor has real work to do.
        js_shadow_slot_set(0, string_bits(young_leaf()));

        // A YOUNG closure owner with an entry: `addr_is_minor_relevant` holds
        // for it, so the prune's re-derivation puts it in `relevant` and the
        // log is required to name it.
        let owner = young_closure();
        crate::closure::closure_set_dynamic_prop(owner, "memo", 42.0);

        // THE SABOTAGE: drop the log while leaving the tables populated —
        // exactly what a writer that publishes without arming would leave
        // behind. Clearing the tables as well would empty `relevant` and the
        // check would return early, i.e. prove nothing.
        crate::closure::test_drop_closure_young_log();

        let _ = gc_collect_minor();

        // Unreachable when the guard works.
        panic!(
            "RULE 2 DID NOT FIRE: the prune accepted a log missing a \
             minor-relevant closure owner"
        );
    }

    let out = std::process::Command::new(std::env::current_exe().expect("current test binary"))
        .arg(RULE2_SABOTAGE_TEST)
        .arg("--exact")
        .arg("--nocapture")
        .env(RULE2_SABOTAGE_ENV, "1")
        .output()
        .expect("launch the isolated rule-2 sabotage child");

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(
        !out.status.success(),
        "the child must FAIL: dropping a logged owner has to trip rule 2.\n{combined}"
    );
    assert!(
        combined.contains("young log for closure.dynamic_props does not name"),
        "the child must fail through the RULE 2 assertion, by its own message, \
         not through some unrelated crash.\n{combined}"
    );
    assert!(
        !combined.contains("RULE 2 DID NOT FIRE"),
        "the prune ran to completion with an incomplete log.\n{combined}"
    );
}
