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
fn young_closure_prop_value_is_moved_through_the_log() {
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
    let row = walk("closure.dynamic_props");
    assert!(
        row.partial,
        "a copying minor must take the young-scoped walk"
    );
    assert!(
        row.visited >= 1,
        "the logged owner must have been visited: {row:?}"
    );
    assert!(
        row.kept >= 1,
        "a survivor still young must stay logged: {row:?}"
    );
}

#[test]
fn young_value_under_an_old_closure_owner_is_logged_by_the_value() {
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
    assert!(walk("closure.dynamic_props").partial);
}

#[test]
fn old_closure_entries_are_skipped_by_a_minor() {
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
    let row = walk("closure.dynamic_props");
    assert!(row.partial);
    assert!(row.table_len >= 2, "{row:?}");
    assert_eq!(
        row.visited, 0,
        "an old owner with no heap values must not be visited by a minor: {row:?}"
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
