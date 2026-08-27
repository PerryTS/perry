//! Death pruning for object-address-keyed side tables (2026-07-09 GC audit,
//! wave 2 batch B — `gc/dead_owner.rs`).
//!
//! Each family test follows the same shape: create an owner, install a side
//! table entry keyed by its address, drop every reference, run a collection,
//! and assert the entry is gone — plus the two safety inverses (a LIVE owner's
//! entries survive; a TENURED owner's entries survive a MINOR trace, which
//! never marks the old generation and therefore proves nothing about it).
//!
//! ★ #7975 — THE PREMISE EVERY "IT IS PRUNED" CASE HERE DEPENDS ON. Arena
//! block reset is all-or-nothing, so `gc::trace` force-MARKS every object in a
//! block that still holds one reachable object
//! (`mark_block_persisting_arena_objects`, `BLOCK_PERSIST_WINDOW`). An owner
//! sharing its block with ANY live object is therefore not dead, and the prune
//! *correctly* keeps its entry — a persisting block cannot recycle the
//! address, so the entry is not stale. A case that lets a live tenant into its
//! block does not test the prune; it fails on it.
//!
//! The tenant that actually turned up is the lazy `globalThis` realm bootstrap
//! (~1.15 MB allocated, ~410 KB live), reached through the **process-global**
//! memoized `Object.prototype` address that `set_property_attrs` and
//! `js_object_set_field_by_name` resolve. It misses once per PROCESS, so which
//! libtest thread pays for it is a scheduling accident: two cases here failed
//! 200/200 alone and 0/200 behind a sibling. Use
//! [`GcTestIsolationGuard::with_realm_bootstrapped`] in any case that reaches a
//! realm-resolving API, and collect through [`full_gc_with_no_block_persistence`]
//! so the premise fails as a premise instead of as the subject.

use super::super::*;
use super::support::*;

fn full_gc() {
    let _ =
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
}

struct ArraySideTableTestGuard;

impl ArraySideTableTestGuard {
    fn new() -> Self {
        crate::array::test_clear_array_named_property_roots();
        crate::map::test_clear_map_iterator_arrays();
        crate::set::test_clear_set_iterator_arrays();
        Self
    }
}

impl Drop for ArraySideTableTestGuard {
    fn drop(&mut self) {
        crate::array::test_clear_array_named_property_roots();
        crate::map::test_clear_map_iterator_arrays();
        crate::set::test_clear_set_iterator_arrays();
    }
}

fn register_array_side_table_scanners() {
    gc_register_mutable_root_scanner(crate::array::scan_template_raw_roots_mut);
    gc_register_mutable_root_scanner(crate::map::scan_map_iterator_array_roots_mut);
    gc_register_mutable_root_scanner(crate::set::scan_set_iterator_array_roots_mut);
}

unsafe fn set_array_named_property(arr: *mut crate::array::ArrayHeader, name: &str, value: f64) {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::array::array_named_property_set(arr, key, value);
}

unsafe fn alloc_nursery_test_array() -> *mut crate::array::ArrayHeader {
    let arr = crate::arena::arena_alloc_gc(
        std::mem::size_of::<crate::array::ArrayHeader>(),
        std::mem::align_of::<crate::array::ArrayHeader>(),
        GC_TYPE_ARRAY,
    ) as *mut crate::array::ArrayHeader;
    (*arr).length = 0;
    (*arr).capacity = 0;
    arr
}

unsafe fn alloc_malloc_test_object() -> *mut crate::object::ObjectHeader {
    let obj = gc_malloc(
        std::mem::size_of::<crate::object::ObjectHeader>(),
        GC_TYPE_OBJECT,
    ) as *mut crate::object::ObjectHeader;
    (*obj).class_id = 0;
    // #8113: zero live slots, so no descriptor is needed — the derived bound
    // for an unstamped receiver is 0, which is the right answer here.
    (*obj).parent_class_id = 0;
    (*obj).meta = std::ptr::null_mut();
    obj
}

/// PREMISE GUARD for the "dead owner ⇒ entry pruned" cases (#7975).
///
/// "The entry is gone" only says something about the death prune when the owner
/// was genuinely DEAD. Arena block reset is all-or-nothing, so
/// `gc::trace::mark_block_persisting_arena_objects` force-MARKS every object in
/// a block that still holds one reachable object — and a force-marked owner is
/// *correctly* left registered, because a block that persists cannot recycle
/// its address, so the entry is not stale. A case whose block picked up an
/// unrelated live tenant therefore fails on the prune assertion and blames the
/// prune for something the prune got right.
///
/// The mark itself is cleared again by the sweep, so nothing about it survives
/// the collection for a test to read. `gc::trace`'s force-mark census is the
/// one quantity that does, and it has exactly one satisfying path: it moves iff
/// some block in the persist window held a reachable object.
///
/// Wrap the collection so the premise fails as a premise, before the subject
/// assertion gets a chance to mis-name it.
fn full_gc_with_no_block_persistence() {
    let force_marks_before = crate::gc::block_persist_force_mark_count();
    full_gc();
    assert_eq!(
        crate::gc::block_persist_force_mark_count(),
        force_marks_before,
        "test premise: block persistence force-marked objects during this \
         collection, so an unrooted owner in one of those blocks is NOT dead \
         and the prune correctly declines to drop its entry. Something \
         reachable shares the owner's arena block — the lazy `globalThis` \
         realm bootstrap is the one that turned up (#7975); see \
         `GcTestIsolationGuard::with_realm_bootstrapped`"
    );
}

fn util_types_is_map_iterator(addr: usize) -> bool {
    crate::object::js_util_types_is_map_iterator(f64::from_bits(ptr_bits(addr))).to_bits()
        == crate::value::TAG_TRUE
}

fn util_types_is_set_iterator(addr: usize) -> bool {
    crate::object::js_util_types_is_set_iterator(f64::from_bits(ptr_bits(addr))).to_bits()
        == crate::value::TAG_TRUE
}

#[test]
fn test_array_named_dead_owner_stops_rooting_value_after_full_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _side_tables = ArraySideTableTestGuard::new();
    register_array_side_table_scanners();
    crate::arena::arena_reset_all_blocks_to_zero();

    let value = unsafe { alloc_malloc_test_object() };
    let owner = unsafe { alloc_nursery_test_array() };
    unsafe {
        set_array_named_property(owner, "payload", f64::from_bits(ptr_bits(value as usize)));
    }

    full_gc();
    assert!(
        !crate::array::test_array_named_property_owner_exists(owner as usize),
        "the first full collection must prune the dead owner"
    );
    assert!(
        malloc_user_ptr_tracked(value as *mut u8),
        "the value was scanned before post-trace pruning and survives that cycle"
    );

    full_gc();
    assert!(
        !malloc_user_ptr_tracked(value as *mut u8),
        "without the dead owner entry, the next full collection must reclaim the value"
    );
}

#[test]
fn test_array_named_dead_owner_cannot_leak_property_across_exact_eden_reuse() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _side_tables = ArraySideTableTestGuard::new();
    register_array_side_table_scanners();
    crate::arena::arena_reset_all_blocks_to_zero();

    let dead = unsafe { alloc_nursery_test_array() };
    unsafe {
        set_array_named_property(dead, "inherited", f64::from_bits(crate::value::TAG_TRUE));
    }
    let dead_addr = dead as usize;

    let _ = gc_collect_minor();
    let replacement = unsafe { alloc_nursery_test_array() };
    assert_eq!(
        replacement as usize, dead_addr,
        "test premise: the copied-minor Eden reset must reuse the exact owner address"
    );
    assert!(
        unsafe { crate::array::array_named_property_get_by_name(replacement, "inherited") }
            .is_none(),
        "an ordinary replacement array must not inherit the dead array's expando"
    );
}

#[test]
fn test_array_named_live_move_rekeys_owner_and_rewrites_object_value() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _side_tables = ArraySideTableTestGuard::new();
    register_array_side_table_scanners();
    crate::arena::arena_reset_all_blocks_to_zero();

    let arr = unsafe { alloc_nursery_test_array() };
    let old_owner = arr as usize;
    let (value, _) = unsafe { alloc_nursery_test_object(0) };
    let old_value = value as usize;
    unsafe {
        set_array_named_property(arr, "kept", f64::from_bits(ptr_bits(old_value)));
    }
    js_shadow_slot_set(0, ptr_bits(old_owner));

    let _ = gc_collect_minor();

    let new_owner = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(new_owner, old_owner, "test premise: the owner must move");
    let new_value_bits = unsafe {
        crate::array::array_named_property_get_by_name(
            new_owner as *const crate::array::ArrayHeader,
            "kept",
        )
    }
    .expect("the moved owner must retain its expando")
    .to_bits();
    let new_value = (new_value_bits & POINTER_MASK) as usize;
    assert_ne!(new_value, old_value, "the object value must be rewritten");
    assert!(crate::arena::pointer_in_nursery(new_value));
    assert!(crate::array::test_array_named_property_owner_exists(
        new_owner
    ));
    assert!(!crate::array::test_array_named_property_owner_exists(
        old_owner
    ));
}

#[test]
fn test_array_named_materialized_iterator_brands_follow_live_moves() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _side_tables = ArraySideTableTestGuard::new();
    register_array_side_table_scanners();

    let map = crate::map::js_map_alloc(0);
    let set = crate::set::js_set_alloc(0);
    let map_iter = crate::map::js_map_entries(map) as usize;
    let set_iter = crate::set::js_set_to_array(set) as usize;
    assert!(util_types_is_map_iterator(map_iter));
    assert!(util_types_is_set_iterator(set_iter));
    js_shadow_slot_set(0, ptr_bits(map_iter));
    js_shadow_slot_set(1, ptr_bits(set_iter));

    let _ = gc_collect_minor();

    let moved_map_iter = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let moved_set_iter = (js_shadow_slot_get(1) & POINTER_MASK) as usize;
    assert_ne!(moved_map_iter, map_iter);
    assert_ne!(moved_set_iter, set_iter);
    assert!(util_types_is_map_iterator(moved_map_iter));
    assert!(util_types_is_set_iterator(moved_set_iter));
    assert!(!util_types_is_map_iterator(map_iter));
    assert!(!util_types_is_set_iterator(set_iter));

    js_shadow_slot_set(0, 0);
    js_shadow_slot_set(1, 0);
    full_gc();
    assert!(!util_types_is_map_iterator(moved_map_iter));
    assert!(!util_types_is_set_iterator(moved_set_iter));
}

#[test]
fn test_array_named_dead_map_iterator_marker_does_not_brand_exact_eden_reuse() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _side_tables = ArraySideTableTestGuard::new();
    register_array_side_table_scanners();
    crate::arena::arena_reset_all_blocks_to_zero();

    let dead_map = crate::map::js_map_alloc(0);
    let dead_map_iter = crate::map::js_map_entries(dead_map) as usize;
    assert!(util_types_is_map_iterator(dead_map_iter));

    let _ = gc_collect_minor();
    let _replacement_map = crate::map::js_map_alloc(0);
    let replacement_map_array = unsafe { alloc_nursery_test_array() };
    assert_eq!(
        replacement_map_array as usize, dead_map_iter,
        "test premise: the copied-minor Eden reset must reuse the exact Map iterator address"
    );
    assert!(!util_types_is_map_iterator(replacement_map_array as usize));

    // Finalize the replacement Map while its header is still intact; a raw
    // arena reset would otherwise strand its external entries allocation.
    full_gc();
}

#[test]
fn test_array_named_dead_set_iterator_marker_does_not_brand_exact_eden_reuse() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _side_tables = ArraySideTableTestGuard::new();
    register_array_side_table_scanners();
    crate::arena::arena_reset_all_blocks_to_zero();

    let dead_set = crate::set::js_set_alloc(0);
    let dead_set_iter = crate::set::js_set_to_array(dead_set) as usize;
    assert!(util_types_is_set_iterator(dead_set_iter));

    let _ = gc_collect_minor();
    let _replacement_set = crate::set::js_set_alloc(0);
    let replacement_set_array = unsafe { alloc_nursery_test_array() };
    assert_eq!(
        replacement_set_array as usize, dead_set_iter,
        "test premise: the copied-minor Eden reset must reuse the exact Set iterator address"
    );
    assert!(!util_types_is_set_iterator(replacement_set_array as usize));

    // Finalize the replacement Set before the test guard tears down state.
    full_gc();
}

#[test]
fn test_dead_owner_descriptor_entries_pruned_on_full_gc() {
    // `set_property_attrs` below resolves the process-global memoized
    // `Object.prototype` address, and a MISS runs the whole lazy `globalThis`
    // bootstrap inside this test — see `with_realm_bootstrapped` (#7975).
    let _guard = GcTestIsolationGuard::with_realm_bootstrapped();
    let (obj, _) = unsafe { alloc_nursery_test_object(0) };
    let addr = obj as usize;
    crate::object::set_property_attrs(
        addr,
        "frozenKey".to_string(),
        crate::object::PropertyAttrs::new(false, true, false),
    );
    crate::object::set_accessor_descriptor(
        addr,
        "acc".to_string(),
        crate::object::AccessorDescriptor { get: 0, set: 0 },
    );
    assert!(crate::object::get_property_attrs(addr, "frozenKey").is_some());

    // No roots: the owner is dead at the full trace.
    full_gc_with_no_block_persistence();

    assert!(
        crate::object::get_property_attrs(addr, "frozenKey").is_none(),
        "dead owner's PROPERTY_DESCRIPTORS entry must be pruned (a fresh \
         object at the recycled address would inherit `writable:false`)"
    );
    assert!(
        crate::object::get_accessor_descriptor(addr, "acc").is_none(),
        "dead owner's ACCESSOR_DESCRIPTORS entry must be pruned"
    );
}

#[test]
fn test_live_owner_descriptor_entries_survive_full_gc() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let (obj, _) = unsafe { alloc_nursery_test_object(0) };
    let addr = obj as usize;
    crate::object::set_property_attrs(
        addr,
        "kept".to_string(),
        crate::object::PropertyAttrs::new(false, true, true),
    );
    js_shadow_slot_set(0, ptr_bits(addr));

    full_gc();

    // Full mark-sweep is non-moving: the rooted owner keeps its address.
    assert!(
        crate::object::get_property_attrs(addr, "kept").is_some(),
        "live (rooted) owner's descriptor entry must survive a full GC"
    );
}

#[test]
fn test_tenured_owner_descriptor_entries_survive_minor_gc() {
    let _guard = GcTestIsolationGuard::new();
    let (obj, _) = unsafe { alloc_old_test_object(0) };
    let addr = obj as usize;
    crate::object::set_property_attrs(
        addr,
        "oldKey".to_string(),
        crate::object::PropertyAttrs::new(false, true, true),
    );

    // MINOR traces never mark the old generation — an unmarked old-gen
    // header proves nothing, so the prune must NOT fire (the audit's
    // central deadness caveat).
    let _ = gc_collect_minor();

    assert!(
        crate::object::get_property_attrs(addr, "oldKey").is_some(),
        "an old-gen owner's descriptor entry must survive a minor GC — \
         minor-trace deadness is not trustworthy for tenured objects"
    );
}

#[test]
fn test_dead_nursery_owner_descriptor_entries_pruned_on_copied_minor() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let (obj, _) = unsafe { alloc_nursery_test_object(0) };
    let addr = obj as usize;
    crate::object::set_property_attrs(
        addr,
        "gone".to_string(),
        crate::object::PropertyAttrs::new(false, true, true),
    );
    js_shadow_slot_set(0, 0);

    let _ = gc_collect_minor();

    assert!(
        crate::object::get_property_attrs(addr, "gone").is_none(),
        "dead from-space owner's descriptor entry must be pruned by the \
         copied-minor pass"
    );
}

#[test]
fn test_dead_symbol_pointer_pruned_live_symbol_survives_full_gc() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let dead_sym = alloc_tracked_test_symbol() as usize;
    let live_sym = alloc_tracked_test_symbol() as usize;
    crate::symbol::test_seed_symbol_pointer_root(dead_sym);
    crate::symbol::test_seed_symbol_pointer_root(live_sym);
    js_shadow_slot_set(0, ptr_bits(live_sym));

    full_gc();

    assert!(
        !crate::symbol::test_symbol_pointer_root_contains(dead_sym),
        "dead symbol's SYMBOL_POINTERS entry must be pruned (js_is_symbol \
         would alias a later allocation at the recycled address)"
    );
    assert!(
        crate::symbol::test_symbol_pointer_root_contains(live_sym),
        "live (rooted) symbol's SYMBOL_POINTERS entry must survive"
    );
}

#[test]
fn test_dead_owner_symbol_property_entries_pruned_on_full_gc() {
    let _guard = GcTestIsolationGuard::new();
    let (obj, _) = unsafe { alloc_nursery_test_object(0) };
    let owner = obj as usize;
    let sym = unsafe { alloc_old_test_symbol() };
    crate::symbol::test_seed_symbol_property_root(owner, sym, crate::value::TAG_TRUE);
    assert!(crate::symbol::test_symbol_property_owner_exists(owner));

    full_gc();

    assert!(
        !crate::symbol::test_symbol_property_owner_exists(owner),
        "dead owner's SYMBOL_PROPERTIES entry must be pruned (its values \
         were strongly rooted by the scanner forever)"
    );
}

/// SABOTAGE CHECK for [`full_gc_with_no_block_persistence`] (#7975). Every
/// "the premise held" verdict it gives is vacuous unless the census it reads
/// can move — CLAUDE.md's fourth way a gate cannot fail.
///
/// Plant the exact shape the #7975 confounder had, minus the realm: one ROOTED
/// object and one unrooted owner in the same arena block. Nothing here resolves
/// `globalThis` (`closure_set_dynamic_prop` does not), so the planted tenant is
/// the only reachable object in the persist window and the census has exactly
/// one satisfying path.
///
/// Both halves are asserted: the census moves, AND the semantic consequence
/// that makes the premise guard necessary in the first place — a force-marked
/// owner is not dead, so the death prune correctly leaves its entry alone.
#[test]
fn test_block_persistence_census_moves_when_a_block_has_a_live_tenant() {
    let _guard = CopyingNurseryTestGuard::new(1);
    crate::arena::arena_reset_all_blocks_to_zero();

    let (live, _) = unsafe { alloc_nursery_test_object(0) };
    js_shadow_slot_set(0, ptr_bits(live as usize));

    let ptr = crate::arena::arena_alloc_gc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        8,
        GC_TYPE_CLOSURE,
    );
    unsafe { init_test_closure(ptr) };
    let dead = ptr as usize;
    crate::closure::closure_set_dynamic_prop(dead, "memo", 42.0);
    assert!(crate::closure::closure_get_own_dynamic_prop(dead, "memo").is_some());

    let force_marks_before = crate::gc::block_persist_force_mark_count();
    full_gc();

    assert!(
        crate::gc::block_persist_force_mark_count() > force_marks_before,
        "block persistence must force-mark the unrooted neighbour of a rooted \
         object — a census that cannot move makes every \
         `full_gc_with_no_block_persistence` verdict in this file vacuous"
    );
    assert!(
        crate::closure::closure_get_own_dynamic_prop(dead, "memo").is_some(),
        "and the consequence the premise guard exists to name: a force-marked \
         owner is NOT dead, so its side-table entry correctly survives — a case \
         that hits this shape is failing on its premise, not on the prune"
    );
}

#[test]
fn test_dead_closure_side_table_entries_pruned_on_full_gc() {
    let _guard = GcTestIsolationGuard::new();
    let ptr = crate::arena::arena_alloc_gc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        8,
        GC_TYPE_CLOSURE,
    );
    unsafe { init_test_closure(ptr) };
    let addr = ptr as usize;
    crate::closure::closure_set_dynamic_prop(addr, "memo", 42.0);
    crate::closure::closure_set_static_prototype(addr, crate::value::TAG_NULL);
    crate::closure::closure_mark_key_deleted(addr, "name");
    assert!(crate::closure::closure_get_own_dynamic_prop(addr, "memo").is_some());

    full_gc();

    assert!(
        crate::closure::closure_get_own_dynamic_prop(addr, "memo").is_none(),
        "dead closure's CLOSURE_PROPS entry must be pruned"
    );
    assert!(
        crate::closure::closure_static_prototype(addr).is_none(),
        "dead closure's CLOSURE_STATIC_PROTOTYPES entry must be pruned"
    );
    assert!(
        !crate::closure::closure_is_key_deleted(addr, "name"),
        "dead closure's CLOSURE_DELETED_KEYS entry must be pruned"
    );
}

/// The per-object sweep arm (`gc_type_clear_dead_payload_side_tables`) was an
/// explicit no-op for `ClosureDynamicProps`; assert it now clears all three
/// tables when the sweep reclaims a dead closure header.
#[test]
fn test_closure_dead_payload_arm_clears_side_tables() {
    let _global = global_side_table_test_lock();
    let owner: usize = 0xC10C_AB1E_0000_2026;
    crate::closure::closure_set_dynamic_prop(owner, "x", 1.0);
    crate::closure::closure_set_static_prototype(owner, crate::value::TAG_NULL);
    crate::closure::closure_mark_key_deleted(owner, "length");

    gc_type_clear_dead_payload_side_tables(GC_TYPE_CLOSURE, owner);

    assert!(crate::closure::closure_get_own_dynamic_prop(owner, "x").is_none());
    assert!(crate::closure::closure_static_prototype(owner).is_none());
    assert!(!crate::closure::closure_is_key_deleted(owner, "length"));
}

/// The `ObjectOverflowFields` dead-payload arm pruned only OVERFLOW_FIELDS;
/// its address-keyed sibling KEYS_INDEX (the key→slot sidecar built for
/// objects past KEYS_INDEX_THRESHOLD own keys) was never removed on death, so
/// entries accumulated forever under recycled addresses — an unbounded leak.
/// Assert the arm now clears the KEYS_INDEX entry alongside the overflow one.
#[test]
fn test_object_dead_payload_arm_clears_keys_index() {
    // #6759 C1: key indexes are shape records keyed on the KEYS_ARRAY
    // address; the per-object dead-payload arm no longer prunes them.
    // The successor guarantee: the dead-owner fan-out drops a shape whose
    // keys_array is dead (memory-only — per-hit content validation covers
    // correctness for anything the prune misses).
    let _global = global_side_table_test_lock();
    let dead_keys: usize = 0x0BEC_1DE0_0000_2026;
    let live_keys: usize = 0x0BEC_1DE0_0000_3026;
    crate::object::test_seed_keys_index_entry(dead_keys);
    crate::object::test_seed_keys_index_entry(live_keys);
    assert!(crate::object::test_keys_index_entry_exists(dead_keys));

    crate::object::shapes::prune_dead_shape_keys(&|addr| addr == dead_keys);

    assert!(
        !crate::object::test_keys_index_entry_exists(dead_keys),
        "a dead keys_array's shape record must be pruned by the dead-owner \
         fan-out (else it leaks forever keyed on the recycled address)"
    );
    assert!(
        crate::object::test_keys_index_entry_exists(live_keys),
        "a live keys_array's shape record must survive the prune"
    );
    crate::object::shapes::shape_drop(live_keys as *const crate::array::ArrayHeader);
    crate::object::shapes::test_drop_shape_descriptors(live_keys);
}

#[test]
fn test_dead_arguments_object_entry_pruned_on_full_gc() {
    // `js_arguments_object_alloc` reaches `js_object_set_field_by_name`, which
    // resolves the process-global memoized `Object.prototype` address; a MISS
    // runs the lazy `globalThis` bootstrap inside this test — see
    // `with_realm_bootstrapped` (#7975).
    let _guard = GcTestIsolationGuard::with_realm_bootstrapped();
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let obj = crate::object::js_arguments_object_alloc(undefined, undefined, 0);
    let addr = obj as usize;
    assert!(crate::object::test_arguments_object_registered(addr));

    full_gc_with_no_block_persistence();

    assert!(
        !crate::object::test_arguments_object_registered(addr),
        "dead arguments object's ARGUMENTS_OBJECTS entry must be pruned \
         (one insert per call of any function referencing `arguments`)"
    );
}

/// The mapped-arguments capture boxes are raw (non-GC) allocations whose
/// pointers now get a strong (validated) visit; the entry itself must be
/// rekeyed when the owning object moves in a copied minor, with the box
/// pointer intact and readable.
#[test]
fn test_arguments_entry_rekeys_and_mapped_box_survives_copied_minor() {
    let _guard = CopyingNurseryTestGuard::new(1);
    gc_register_mutable_root_scanner(crate::object::scan_arguments_object_roots_mut);

    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let obj = crate::object::js_arguments_object_alloc(undefined, undefined, 0);
    let addr = obj as usize;
    let boxed = crate::r#box::js_box_alloc(42.0);
    crate::object::js_arguments_object_map_index(obj, 0, boxed);
    assert_eq!(
        crate::object::test_arguments_mapped_box(addr, 0),
        Some(boxed as usize)
    );
    js_shadow_slot_set(0, ptr_bits(addr));

    let _ = gc_collect_minor();

    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(moved, addr, "test premise: the owner must actually move");
    assert!(
        crate::object::test_arguments_object_registered(moved),
        "arguments metadata must be rekeyed to the owner's post-move address"
    );
    assert!(
        !crate::object::test_arguments_object_registered(addr),
        "the stale pre-move key must be gone"
    );
    assert_eq!(
        crate::object::test_arguments_mapped_box(moved, 0),
        Some(boxed as usize),
        "mapped box pointer must survive the move (boxes are non-GC \
         allocations; the strong visit is a validated no-op for them)"
    );
    assert_eq!(crate::r#box::js_box_get(boxed), 42.0);
}

#[test]
fn test_dead_owner_prototype_vm_expando_and_filehandle_entries_pruned() {
    let _guard = GcTestIsolationGuard::new();

    // OBJECT_PROTOTYPES (residual registry, #6759 B): shaped objects now
    // store their recorded prototype in the per-object meta record, which
    // dies WITH the owner (no prune needed — see
    // `test_object_meta_prototype_survives_copied_minor_move`). The
    // registry still backs non-object owners, so exercise the prune with a
    // TYPED-ARRAY-tagged owner. (Not a real array: retargeting one flips
    // the process-wide `ARRAY_TARGET_PROTO_RECORDED` latch, permanently
    // standing down the typed-feedback array fast paths every later
    // `typed_feedback` guard test in this process asserts.)
    let proto_owner = crate::arena::arena_alloc_gc(
        std::mem::size_of::<crate::object::ObjectHeader>(),
        8,
        GC_TYPE_TYPED_ARRAY,
    ) as usize;
    crate::object::prototype_chain::object_set_static_prototype(
        proto_owner,
        crate::value::TAG_NULL,
    );
    assert!(crate::object::prototype_chain::object_static_prototype(proto_owner).is_some());

    // EXOTIC_EXPANDO: expando on a (movable, here old-gen) Promise cell.
    let promise = unsafe { alloc_old_test_promise() } as usize;
    crate::object::exotic_expando::test_seed_exotic_expando_entry(
        promise,
        "status",
        crate::value::TAG_TRUE,
    );
    assert!(crate::object::exotic_expando::test_exotic_expando_entry_exists(promise));

    // VM_SCRIPTS: retains full source text per vm.Script.
    let (vm_owner, _) = unsafe { alloc_nursery_test_object(0) };
    let vm_owner = vm_owner as usize;
    crate::node_vm::test_seed_vm_script_entry(vm_owner, "while(true){}");
    assert!(crate::node_vm::test_vm_script_entry_exists(vm_owner));

    // FILEHANDLE_OBJECT_FDS: FileHandle object address → synthetic fd.
    let (fh_owner, _) = unsafe { alloc_nursery_test_object(0) };
    let fh_owner = fh_owner as usize;
    crate::fs::test_seed_filehandle_fd_entry(fh_owner, 4242);
    assert!(crate::fs::test_filehandle_fd_entry_exists(fh_owner));

    full_gc();

    assert!(
        crate::object::prototype_chain::object_static_prototype(proto_owner).is_none(),
        "dead owner's OBJECT_PROTOTYPES entry must be pruned"
    );
    assert!(
        !crate::object::exotic_expando::test_exotic_expando_entry_exists(promise),
        "dead promise's EXOTIC_EXPANDO entry must be pruned"
    );
    assert!(
        !crate::node_vm::test_vm_script_entry_exists(vm_owner),
        "dead owner's VM_SCRIPTS entry (full source text!) must be pruned"
    );
    assert!(
        !crate::fs::test_filehandle_fd_entry_exists(fh_owner),
        "dead owner's FILEHANDLE_OBJECT_FDS entry must be pruned"
    );
}

/// `DOM_EXCEPTION_ERRORS` had zero removals; it is now folded into the
/// `ErrorSideTables` death cleanup that every dead error already runs.
#[test]
fn test_dom_exception_set_cleared_with_error_side_tables() {
    let _global = global_side_table_test_lock();
    let addr: usize = 0xD0E0_0000_0000_2026;
    crate::event_target::test_seed_dom_exception_error(addr);
    assert!(crate::event_target::test_dom_exception_error_registered(
        addr
    ));

    crate::node_submodules::diagnostics_gc::error_side_tables_clear_dead(addr);

    assert!(
        !crate::event_target::test_dom_exception_error_registered(addr),
        "dead error must be removed from DOM_EXCEPTION_ERRORS by the error \
         side-table death cleanup"
    );
}

/// #6759 Phase B: a shaped object's recorded `[[Prototype]]` lives in its
/// per-object `ObjectMeta` record. A copied minor moves the owner, the meta
/// record, AND the prototype object; the header's meta edge and the
/// record's prototype slot must both be rewritten so the moved owner still
/// resolves the moved prototype.
#[test]
fn test_object_meta_prototype_survives_copied_minor_move() {
    let _guard = CopyingNurseryTestGuard::new(2);

    let (owner, _) = unsafe { alloc_nursery_test_object(0) };
    let (proto, _) = unsafe { alloc_nursery_test_object(0) };
    let old_owner = owner as usize;
    let old_proto = proto as usize;
    crate::object::prototype_chain::object_set_static_prototype(old_owner, ptr_bits(old_proto));
    assert_eq!(
        crate::object::prototype_chain::object_static_prototype(old_owner),
        Some(ptr_bits(old_proto)),
        "test premise: the meta-resident prototype reads back before the GC"
    );
    assert!(
        crate::object::prototype_chain::object_has_prototype_override(old_owner),
        "test premise: the per-instance override bit lives in the meta record"
    );
    js_shadow_slot_set(0, ptr_bits(old_owner));
    js_shadow_slot_set(1, ptr_bits(old_proto));

    let _ = gc_collect_minor();

    let new_owner = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let new_proto = (js_shadow_slot_get(1) & POINTER_MASK) as usize;
    assert_ne!(new_owner, old_owner, "test premise: the owner must move");
    assert_ne!(new_proto, old_proto, "test premise: the proto must move");
    let recorded = crate::object::prototype_chain::object_static_prototype(new_owner)
        .expect("the moved owner must still resolve its recorded prototype via its meta record");
    assert_eq!(
        (recorded & POINTER_MASK) as usize,
        new_proto,
        "the meta record's prototype slot must be rewritten to the moved proto"
    );
    assert!(
        crate::object::prototype_chain::object_has_prototype_override(new_owner),
        "the non-pointer meta flags must travel with the copied record"
    );

    js_shadow_slot_set(0, 0);
    js_shadow_slot_set(1, 0);
}

/// A class-evaluation object may be reachable only through an instance's
/// hidden ObjectMeta brand. The edge must retain and rewrite that class object
/// when a copied minor moves the owner, its metadata, and the brand together.
#[test]
fn test_object_meta_private_evaluation_brand_survives_copied_minor_move() {
    let _guard = CopyingNurseryTestGuard::new(1);

    let (owner, _) = unsafe { alloc_nursery_test_object(0) };
    let (class, _) = unsafe { alloc_nursery_test_object(0) };
    unsafe {
        crate::object::js_object_mark_class(class as i64);
        let meta = crate::object::object_meta_ensure(owner);
        (*meta).private_evaluation_brand = ptr_bits(class as usize);
    }
    let old_owner = owner as usize;
    let old_class = class as usize;
    js_shadow_slot_set(0, ptr_bits(old_owner));

    let _ = gc_collect_minor();

    let new_owner = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(new_owner, old_owner, "test premise: the owner must move");
    let brand = unsafe {
        let meta = (*(new_owner as *mut crate::object::ObjectHeader)).meta;
        assert!(
            !meta.is_null(),
            "the moved owner must retain its meta record"
        );
        (*meta).private_evaluation_brand
    };
    let new_class = (brand & POINTER_MASK) as usize;
    assert_ne!(new_class, old_class, "test premise: the brand must move");
    assert!(
        crate::object::is_class_object_value(f64::from_bits(brand)),
        "the metadata edge must retain and rewrite the class evaluation brand"
    );

    js_shadow_slot_set(0, 0);
}

/// #6759 Phase C2: the per-key descriptor summary in the meta record gates
/// table probes — exactly (no false negatives) for installed keys, and
/// authoritatively negative for a fresh owner and for keys whose Bloom bit
/// is clear. Non-meta-capable owners (handle-band ids) stay on the
/// conservative probe-always arm, so their installs still round-trip.
#[test]
fn test_descriptor_meta_summary_gates_probes() {
    // NOTE: the guard already takes the process-global side-table lock —
    // taking `global_side_table_test_lock()` here too self-deadlocks.
    let _guard = GcTestIsolationGuard::new();

    unsafe {
        let (owner, _) = alloc_nursery_test_object(0);
        let addr = owner as usize;

        // Fresh meta-capable owner: null meta is an authoritative miss.
        assert!(
            !crate::object::descriptor_state::may_have_descriptor_entry(addr, "x", false),
            "fresh owner must report no possible attr entry"
        );
        assert!(
            !crate::object::descriptor_state::may_have_descriptor_entry(addr, "x", true),
            "fresh owner must report no possible accessor entry"
        );
        assert!(
            !crate::object::owner_may_have_descriptor_entries(addr, false),
            "fresh owner must report no possible entries at all"
        );

        crate::object::set_property_attrs(
            addr,
            "x".to_string(),
            crate::object::PropertyAttrs::new(false, true, true),
        );
        assert!(
            crate::object::descriptor_state::may_have_descriptor_entry(addr, "x", false),
            "installed key's bit must be set"
        );
        assert!(
            crate::object::get_property_attrs(addr, "x").is_some_and(|a| !a.writable()),
            "gated getter must still return the installed attrs"
        );
        assert!(
            crate::object::get_property_attrs(addr, "unrelated").is_none(),
            "un-installed key must miss through the gate"
        );
        // Exact negative when the bits don't collide; a collision only
        // costs a (missing) probe, which the getter assertion above covers.
        let x_bit = crate::object::descriptor_state::test_descriptor_key_bit("x");
        let other_bit = crate::object::descriptor_state::test_descriptor_key_bit("unrelated");
        if x_bit != other_bit {
            assert!(
                !crate::object::descriptor_state::may_have_descriptor_entry(
                    addr,
                    "unrelated",
                    false
                ),
                "non-colliding un-installed key must be a summary miss"
            );
        }
        // The attr install must not set the ACCESSOR word.
        if x_bit != 0 {
            assert!(
                !crate::object::descriptor_state::may_have_descriptor_entry(addr, "x", true),
                "attr install must not claim a possible accessor entry"
            );
        }

        // Handle-band owner (no GC header): conservative arm, still works.
        let handle = 0x400usize;
        assert!(
            crate::object::descriptor_state::may_have_descriptor_entry(handle, "x", false),
            "non-meta-capable owner must stay conservative"
        );
        crate::object::set_property_attrs(
            handle,
            "h".to_string(),
            crate::object::PropertyAttrs::new(false, true, true),
        );
        assert!(
            crate::object::get_property_attrs(handle, "h").is_some_and(|a| !a.writable()),
            "handle-band install must round-trip via the conservative arm"
        );
        crate::object::clear_property_attrs(handle, "h");
    }
}

/// #6759 Phase C2: the summary bits live in the meta record, which moves
/// WITH its owner on a copied minor while `scan_descriptor_roots_mut`
/// rekeys the table entry to the owner's new address — the gated getter
/// must still resolve the entry at the moved address.
#[test]
fn test_descriptor_meta_summary_survives_copied_minor_move() {
    let _guard = CopyingNurseryTestGuard::new(2);
    // The scoped registry starts empty — install the scanner that rekeys
    // descriptor-table owner addresses on evacuation.
    gc_register_mutable_root_scanner(crate::object::descriptor_state::scan_descriptor_roots_mut);

    let (owner, _) = unsafe { alloc_nursery_test_object(0) };
    let old_addr = owner as usize;
    crate::object::set_property_attrs(
        old_addr,
        "ro".to_string(),
        crate::object::PropertyAttrs::new(false, true, false),
    );
    js_shadow_slot_set(0, ptr_bits(old_addr));

    let _ = gc_collect_minor();

    let new_addr = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(new_addr, old_addr, "test premise: the owner must move");
    assert!(
        crate::object::descriptor_state::may_have_descriptor_entry(new_addr, "ro", false),
        "summary bits must travel with the moved owner's meta record"
    );
    let attrs = crate::object::get_property_attrs(new_addr, "ro")
        .expect("rekeyed descriptor entry must resolve at the moved address");
    assert!(
        !attrs.writable() && !attrs.configurable(),
        "moved owner must keep its installed attributes"
    );
    assert!(
        crate::object::get_property_attrs(new_addr, "other").is_none(),
        "un-installed key must still miss at the moved address"
    );

    crate::object::clear_property_attrs(new_addr, "ro");
    js_shadow_slot_set(0, 0);
}

/// #8067: an owned keys array's append-reallocation may migrate its validated
/// slot-index accelerator, but must neither repoint nor eagerly delete the old
/// immutable descriptor. A sibling naming it remains valid; otherwise weak
/// post-trace pruning eventually retires it.
#[test]
fn test_shape_record_migrates_on_owned_grow() {
    let _global = global_side_table_test_lock();
    let old_addr: usize = 0xC3A0_0000_0000_1010;
    let new_addr: usize = 0xC3A0_0000_0000_2020;
    crate::object::shapes::test_seed_shape_entry(old_addr);
    let id = crate::object::shapes::test_shape_id_for_keys(old_addr)
        .expect("seeded entry must have an id");
    assert!(id != 0, "shape ids are 1-based");

    crate::object::shapes::shape_keys_grown(old_addr, new_addr as *const crate::array::ArrayHeader);

    assert!(
        !crate::object::shapes::test_shape_entry_exists(old_addr),
        "grown-away address must no longer hold the record"
    );
    assert_eq!(
        crate::object::shapes::test_shape_id_for_keys(new_addr),
        None,
        "an append-reallocation must not repoint the old immutable descriptor"
    );
    assert_eq!(
        crate::object::shapes::shape_descriptor_by_id(id)
            .expect("grow must not delete a potentially sibling-owned descriptor")
            .keys,
        old_addr as u64,
        "an append-reallocation repointed the old immutable descriptor"
    );
    // Cleanup so the seeded address can't leak into later tests.
    crate::object::shapes::shape_drop(new_addr as *const crate::array::ArrayHeader);
    crate::object::shapes::test_drop_shape_descriptors(old_addr);
}

/// #6759 Phase C3a: GC evacuation MOVES a live keys array — the shape
/// table's metadata-rewrite scanner must rekey the record to the array's
/// to-space address (same pattern as the descriptor owner rekey), so a
/// wide object's slot map survives a copied minor.
#[test]
fn test_shape_record_rekeys_on_copied_minor_move() {
    let _guard = CopyingNurseryTestGuard::new(2);
    // The scoped registry starts empty — install the C3a rekey scanner.
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);

    let keys = unsafe { alloc_nursery_test_array() };
    let old_addr = keys as usize;
    crate::object::shapes::test_seed_shape_entry(old_addr);
    let id = crate::object::shapes::test_shape_id_for_keys(old_addr)
        .expect("seeded entry must have an id");
    js_shadow_slot_set(0, ptr_bits(old_addr));

    let _ = gc_collect_minor();

    let new_addr = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(new_addr, old_addr, "test premise: the keys array must move");
    assert!(
        !crate::object::shapes::test_shape_entry_exists(old_addr),
        "from-space address must no longer key the record"
    );
    assert_eq!(
        crate::object::shapes::test_shape_id_for_keys(new_addr),
        Some(id),
        "the shape record must be rekeyed to the moved keys array"
    );

    crate::object::shapes::shape_drop(new_addr as *const crate::array::ArrayHeader);
    crate::object::shapes::test_drop_shape_descriptors(new_addr);
    js_shadow_slot_set(0, 0);
}

/// #8067: the by-id table is weak. Churning shapes without any live object
/// owners must return the descriptor census to baseline after a full trace;
/// otherwise the descriptor's keys copy would make historical arrays immortal.
#[test]
fn test_dead_shape_descriptor_churn_returns_to_baseline_after_full_gc() {
    let _guard = GcTestIsolationGuard::new();
    crate::object::shapes::test_clear_shape_table();
    let baseline = crate::object::shapes::test_shape_descriptor_count();
    let mut ids = Vec::new();
    for _ in 0..32 {
        let keys = unsafe { alloc_nursery_test_array() };
        ids.push(
            crate::object::shapes::shape_descriptor_ensure(keys, 0, 0)
                .expect("shape range unexpectedly exhausted"),
        );
    }
    assert_eq!(
        crate::object::shapes::test_shape_descriptor_count(),
        baseline + ids.len(),
        "test premise: churn must publish one descriptor per distinct keys array"
    );

    full_gc_with_no_block_persistence();

    assert_eq!(
        crate::object::shapes::test_shape_descriptor_count(),
        baseline,
        "weak descriptor table retained dead shape keys"
    );
    assert!(
        ids.into_iter()
            .all(|id| crate::object::shapes::shape_descriptor_by_id(id).is_none()),
        "a dead shape id still resolved after its keys array was reclaimed"
    );
}

/// #8112: the DESCRIPTOR record is the strong edge and the rewritten location;
/// Two siblings share one descriptor — and therefore one edge — so after
/// copied-minor evacuation that one record must be rewritten exactly once.
#[test]
fn test_shared_live_shape_descriptor_survives_and_rekeys_once() {
    let _guard = CopyingNurseryTestGuard::new(2);
    crate::object::shapes::test_clear_shape_table();
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);

    let keys = unsafe { alloc_nursery_test_array() };
    let old_keys = keys as usize;
    let id = crate::object::shapes::shape_descriptor_ensure(keys, 0, 0)
        .expect("shape range unexpectedly exhausted");
    let (a, _) = unsafe { alloc_nursery_test_object(0) };
    let (b, _) = unsafe { alloc_nursery_test_object(0) };
    unsafe {
        (*a).parent_class_id = id;
        (*b).parent_class_id = id;
    }
    assert_eq!(
        crate::gc::test_gc_rewrite_slot_addresses(a as usize),
        Some(vec![crate::object::shapes::shape_descriptor_keys_slot(id)
            .expect("a table-resident descriptor exposes its keys word")
            as usize,]),
        "#8047: a stamped object enumerates only the authoritative descriptor edge"
    );
    assert_eq!(
        crate::gc::test_gc_rewrite_slot_addresses(b as usize).map(|slots| slots[0]),
        crate::gc::test_gc_rewrite_slot_addresses(a as usize).map(|slots| slots[0]),
        "#8112: siblings of one shape must share ONE keys edge"
    );
    js_shadow_slot_set(0, ptr_bits(a as usize));
    js_shadow_slot_set(1, ptr_bits(b as usize));

    let _ = gc_collect_minor();

    let a_after = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::object::ObjectHeader;
    let b_after = (js_shadow_slot_get(1) & POINTER_MASK) as *mut crate::object::ObjectHeader;
    let descriptor = crate::object::shapes::shape_descriptor_by_id(id)
        .expect("live shared descriptor disappeared during evacuation");
    unsafe {
        assert_eq!((*a_after).parent_class_id, id);
        assert_eq!((*b_after).parent_class_id, id);
        assert_eq!(
            crate::object::object_keys_array(a_after),
            crate::object::object_keys_array(b_after)
        );
        assert_ne!(crate::object::object_keys_array(a_after) as usize, old_keys);
        assert_eq!(
            descriptor.keys,
            crate::object::object_keys_array(a_after) as u64
        );
    }
    assert_eq!(
        crate::object::shapes::test_shape_descriptor_count(),
        1,
        "two live siblings must retain exactly their one shared descriptor"
    );
    crate::object::shapes::shape_drop(descriptor.keys as usize as *const crate::array::ArrayHeader);
    crate::object::shapes::test_drop_shape_descriptors(descriptor.keys as usize);
    js_shadow_slot_set(0, 0);
    js_shadow_slot_set(1, 0);
}

/// #8074 review, retired by #8112 and kept as its regression guard.
///
/// The hazard was real under the old model: a forwarded array's from-space
/// payload holds its forwarding address, not a usable `(length, capacity)`
/// pair, and the post-visit callback CAPTURED descriptor facts from the header
/// edge — so a stale capacity word could truncate the logical count and make
/// the rewrite fail closed. There is no fact capture any more. The visitor
/// writes the record it was handed, reading no `ArrayHeader` field at all, so
/// this fixture now asserts the structural reason the hazard cannot recur:
/// rewriting through the enumerated descriptor slot lands exactly, with every
/// other fact untouched, while the array's own length/capacity words say
/// something impossible.
#[test]
fn test_forwarded_keys_capacity_cannot_disturb_the_descriptor_rewrite() {
    let _guard = GcTestIsolationGuard::new();
    crate::object::shapes::test_clear_shape_table();

    let old_keys = unsafe { alloc_nursery_test_array() };
    let live_keys = unsafe { alloc_nursery_test_array() };
    // `set_forwarding_address` stores this pointer over the old length/capacity
    // pair. Pick a logical count one above the resulting stale capacity word,
    // so a re-derivation from the array would deterministically truncate it.
    let stale_capacity = ((live_keys as u64) >> 32) as u32;
    let logical_key_count = stale_capacity
        .checked_add(1)
        .expect("tracked array address unexpectedly fills the high u32");
    unsafe {
        (*old_keys).length = logical_key_count;
        (*old_keys).capacity = logical_key_count;
        (*live_keys).length = logical_key_count;
        (*live_keys).capacity = logical_key_count;
    }
    let id = crate::object::shapes::shape_descriptor_ensure(old_keys, logical_key_count, 0)
        .expect("shape range unexpectedly exhausted");
    let (owner, _) = unsafe { alloc_nursery_test_object(0) };
    unsafe {
        (*owner).parent_class_id = id;
    }

    let old_header = unsafe { header_from_user_ptr(old_keys.cast()) } as *mut GcHeader;
    let old_flags = unsafe { (*old_header).gc_flags };
    let old_payload = unsafe { *(old_keys as *const u64) };
    unsafe {
        set_forwarding_address(old_header, live_keys.cast());
        assert_eq!((*old_keys).capacity, stale_capacity);
        assert!(
            (*old_keys).capacity < logical_key_count,
            "test premise: the stale capacity would truncate the live count"
        );
    }

    let owner_header = unsafe { header_from_user_ptr(owner.cast()) } as *mut GcHeader;
    let descriptor_keys_slot = crate::object::shapes::shape_descriptor_keys_slot(id)
        .expect("a table-resident descriptor exposes its keys word");
    let mut rewritten = 0usize;
    unsafe {
        visit_gc_layout_slot_descriptors(owner_header, &mut |descriptor| {
            descriptor.visit_slots(&mut |slot| {
                if slot.slot == descriptor_keys_slot {
                    *slot.slot = live_keys as u64;
                    rewritten += 1;
                }
            });
        });
    }
    assert_eq!(
        rewritten, 1,
        "the authoritative descriptor edge must be enumerated exactly once"
    );
    let descriptor = crate::object::shapes::shape_descriptor_by_id(id)
        .expect("rewritten live descriptor disappeared");
    assert_eq!(descriptor.keys, live_keys as u64);
    assert_eq!(
        descriptor.logical_key_count, logical_key_count,
        "#8112: nothing in the visit re-derives a count from the array, so the \
         stale capacity word cannot truncate it"
    );
    assert_eq!(descriptor.live_inline_slot_count, 0);

    unsafe {
        *(old_keys as *mut u64) = old_payload;
        (*old_header).gc_flags = old_flags;
    }
    crate::object::shapes::test_clear_shape_table();
}

/// DirtyHeaderSlotScan retains enumerated raw slot pointers between budgeted
/// work units, and descriptor-table growth in that mutator window must not
/// invalidate any saved pointer. #8067 answered that by refusing to enumerate
/// the descriptor at all; #8112 answers it by BOXING the record, so the map
/// may rehash freely while every enumerated `keys` word stays put. This test
/// is the difference between those two answers: it saves the enumeration,
/// forces a thousand insertions, and demands the identical addresses back.
#[test]
fn test_deferred_shape_slot_enumeration_survives_descriptor_table_reallocation() {
    let _guard = GcTestIsolationGuard::new();
    crate::object::shapes::test_clear_shape_table();
    let frame = js_shadow_frame_push(1);

    let keys_before = unsafe { alloc_nursery_test_array() };
    js_shadow_slot_set(0, ptr_bits(keys_before as usize));
    let id = crate::object::shapes::shape_descriptor_ensure(keys_before, 0, 0)
        .expect("shape range unexpectedly exhausted");
    let (owner, _) = unsafe { alloc_nursery_test_object(0) };
    unsafe {
        (*owner).parent_class_id = id;
    }
    js_shadow_slot_set(0, ptr_bits(owner as usize));
    let saved_slots = crate::gc::test_gc_rewrite_slot_addresses(owner as usize)
        .expect("tracked object must have a rewrite descriptor");
    let descriptor_keys_slot = crate::object::shapes::shape_descriptor_keys_slot(id)
        .expect("a table-resident descriptor exposes its keys word")
        as usize;
    assert_eq!(
        saved_slots,
        vec![descriptor_keys_slot],
        "#8047: the enumeration contains only the stable descriptor record"
    );

    for i in 0..1024usize {
        let fake_keys = 0x8067_1000_0000_0000usize + i * 0x1000;
        crate::object::shapes::shape_descriptor_ensure(fake_keys as *const _, 0, 0)
            .expect("shape range unexpectedly exhausted during reallocation fixture");
    }
    let owner_after = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::object::ObjectHeader;
    let slots_after = crate::gc::test_gc_rewrite_slot_addresses(owner_after as usize)
        .expect("rooted object must remain enumerable after table growth");
    assert_eq!(
        slots_after,
        vec![descriptor_keys_slot],
        "descriptor-table growth moved a keys word that deferred dirty-page \
         work may still be holding — the box did not keep the record put"
    );
    assert_eq!(
        crate::object::shapes::shape_descriptor_keys_slot(id).map(|slot| slot as usize),
        Some(descriptor_keys_slot),
        "1024 insertions rehashed the map and moved the record with it"
    );

    js_shadow_slot_set(0, 0);
    js_shadow_frame_pop(frame);
    crate::object::shapes::test_clear_shape_table();
}

/// #6759 Phase B: the meta record is kept alive by its owner (the header
/// edge is a traced child slot) across a full non-moving collection, and an
/// explicit-null recording is preserved.
#[test]
fn test_object_meta_null_prototype_survives_full_gc_on_live_owner() {
    let _guard = GcTestIsolationGuard::new();

    let (owner, _) = unsafe { alloc_nursery_test_object(0) };
    let addr = owner as usize;
    crate::object::prototype_chain::object_set_static_prototype(addr, crate::value::TAG_NULL);
    js_shadow_slot_set(0, ptr_bits(addr));

    full_gc();

    assert_eq!(
        crate::object::prototype_chain::object_static_prototype(addr),
        Some(crate::value::TAG_NULL),
        "a live (rooted) owner's meta record — and its explicit-null \
         prototype — must survive a full collection"
    );
    js_shadow_slot_set(0, 0);
}

// ── FUNCTION_CLASS_IDS (#8040) ──────────────────────────────────────────────
//
// The synthetic-class table is keyed by a function value's NaN-boxed closure
// ADDRESS, and — unlike every other table in this file — it is *rekeyed* when
// its key object moves (`scan_function_class_id_keys_mut`). So a key that
// outlives its closure does not merely leak: the next collection follows the
// forwarding pointer of whatever object now occupies the address and rebinds
// the class id onto it.

fn alloc_nursery_test_closure() -> usize {
    let ptr = crate::arena::arena_alloc_gc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        8,
        GC_TYPE_CLOSURE,
    );
    unsafe { init_test_closure(ptr) };
    ptr as usize
}

#[test]
fn test_dead_function_class_id_key_pruned_on_full_gc() {
    let _guard = GcTestIsolationGuard::new();
    crate::object::test_clear_class_side_table_roots();

    let addr = alloc_nursery_test_closure();
    crate::object::test_seed_function_class_id_key(ptr_bits(addr), 0x8200_8040);
    assert_eq!(
        crate::object::function_class_id(f64::from_bits(ptr_bits(addr))),
        0x8200_8040,
        "test premise: the synthetic class id must be installed"
    );

    full_gc();

    assert_eq!(
        crate::object::test_function_class_id_key_for_class(0x8200_8040),
        0,
        "a dead closure's FUNCTION_CLASS_IDS key must be pruned"
    );
}

#[test]
fn test_live_function_class_id_key_survives_and_is_rekeyed_on_copied_minor() {
    let _guard = CopyingNurseryTestGuard::new(1);
    crate::object::test_clear_class_side_table_roots();
    gc_register_mutable_root_scanner(crate::object::scan_class_side_table_roots_mut);

    let addr = alloc_nursery_test_closure();
    js_shadow_slot_set(0, ptr_bits(addr));
    crate::object::test_seed_function_class_id_key(ptr_bits(addr), 0x8200_8041);

    let _ = gc_collect_minor();

    let moved = js_shadow_slot_get(0);
    assert_ne!(moved, ptr_bits(addr), "test premise: the closure must move");
    assert_eq!(
        crate::object::test_function_class_id_key_for_class(0x8200_8041),
        moved,
        "a LIVE closure's key must be rekeyed to its new address, not pruned"
    );
}

/// The #8040 shape, end to end: a dead key that survives one collection is
/// re-pointed at the next tenant of its address, and the collection after that
/// follows THAT object's forwarding pointer.
#[test]
fn test_dead_function_class_id_key_cannot_follow_exact_eden_reuse() {
    let _guard = CopyingNurseryTestGuard::new(1);
    crate::object::test_clear_class_side_table_roots();
    gc_register_mutable_root_scanner(crate::object::scan_class_side_table_roots_mut);
    crate::arena::arena_reset_all_blocks_to_zero();

    let dead_addr = alloc_nursery_test_closure();
    crate::object::test_seed_function_class_id_key(ptr_bits(dead_addr), 0x8200_8042);

    let _ = gc_collect_minor();

    let replacement = alloc_nursery_test_closure();
    assert_eq!(
        replacement, dead_addr,
        "test premise: the copied-minor Eden reset must reuse the exact address"
    );
    js_shadow_slot_set(0, ptr_bits(replacement));
    assert_eq!(
        crate::object::function_class_id(f64::from_bits(ptr_bits(replacement))),
        0,
        "an unrelated closure allocated at the recycled address must not inherit \
         the dead function's synthetic class id"
    );

    let _ = gc_collect_minor();

    let moved = js_shadow_slot_get(0);
    assert_ne!(
        moved,
        ptr_bits(replacement),
        "test premise: the replacement must move"
    );
    assert_eq!(
        crate::object::function_class_id(f64::from_bits(moved)),
        0,
        "and the stale key must not have been REKEYED onto the replacement's new \
         address by the metadata rewrite pass"
    );
    assert_eq!(
        crate::object::test_function_class_id_key_count(),
        0,
        "no FUNCTION_CLASS_IDS entry may outlive its closure"
    );
}

// ── The four tables the #8174 audit found REKEYED with no death story ───────
//
// #8168 wired `FUNCTION_CLASS_IDS` into the fan-out above. The registry gate
// that came with #8174 (`scripts/gc_rekeyed_key_tables.py`) then enumerated
// every `visit_metadata_*` site in the tree and asked the same question of each
// — and found four more tables with no prune and no rooting, plus a fifth
// (`SYMBOL_ACCESSOR_PROPERTIES`) whose sibling tables were pruned while it was
// not. Each is the same hazard: the key is rewritten if the object moves and
// deliberately not marked, so it can go stale, and a stale key on a REKEYED
// table is followed into recycled bytes.
//
// Every case below asserts the prune FIRES (a dead owner's entry is gone) and
// is paired with the inverse (a live owner's entry is not), because a prune
// that drops live entries is a worse bug than the one it fixes.

fn alloc_nursery_test_object_addr() -> usize {
    let (obj, _) = unsafe { alloc_nursery_test_object(0) };
    obj as usize
}

/// #8190. `CONSOLE_INSTANCES` is keyed by a `new console.Console(...)`
/// instance's heap address and rekeyed by
/// `scan_console_log_singleton_roots_mut`.
#[test]
fn test_dead_console_instance_owner_pruned_on_full_gc() {
    let _guard = GcTestIsolationGuard::new();

    let dead = alloc_nursery_test_object_addr();
    crate::builtins::test_seed_console_instance(dead);
    assert_eq!(
        crate::builtins::test_console_instance_count(),
        1,
        "test premise: the instance entry must be installed"
    );

    full_gc();

    assert_eq!(
        crate::builtins::test_console_instance_count(),
        0,
        "a dead console instance's CONSOLE_INSTANCES entry must be pruned"
    );
}

#[test]
fn test_live_console_instance_owner_survives_full_gc() {
    let _guard = CopyingNurseryTestGuard::new(1);

    let live = alloc_nursery_test_object_addr();
    js_shadow_slot_set(0, ptr_bits(live));
    crate::builtins::test_seed_console_instance(live);

    full_gc();

    assert_eq!(
        crate::builtins::test_console_instance_count(),
        1,
        "a ROOTED console instance's entry must survive — a prune that drops \
         live entries is worse than the stale key it removes"
    );
    js_shadow_slot_set(0, 0);
}

/// #8191. `BOXED_PRIMITIVE_PAYLOADS` is keyed by a boxed `Number`/`String`/…
/// wrapper's `ObjectHeader` address and rekeyed by
/// `scan_boxed_primitive_payload_roots_mut`.
#[test]
fn test_dead_boxed_primitive_payload_owner_pruned_on_full_gc() {
    let _guard = GcTestIsolationGuard::new();

    let dead = alloc_nursery_test_object_addr();
    crate::builtins::test_seed_boxed_primitive_payload(dead, 42f64.to_bits());
    assert_eq!(
        crate::builtins::test_boxed_primitive_payload_count(),
        1,
        "test premise: the payload entry must be installed"
    );

    full_gc();

    assert_eq!(
        crate::builtins::test_boxed_primitive_payload_count(),
        0,
        "a dead wrapper's BOXED_PRIMITIVE_PAYLOADS entry must be pruned"
    );
}

#[test]
fn test_live_boxed_primitive_payload_owner_survives_full_gc() {
    let _guard = CopyingNurseryTestGuard::new(1);

    let live = alloc_nursery_test_object_addr();
    js_shadow_slot_set(0, ptr_bits(live));
    crate::builtins::test_seed_boxed_primitive_payload(live, 42f64.to_bits());

    full_gc();

    assert_eq!(
        crate::builtins::test_boxed_primitive_payload_count(),
        1,
        "a ROOTED wrapper's payload entry must survive"
    );
    js_shadow_slot_set(0, 0);
}

/// #8192. A transition-cache entry carries one metadata-only key — the
/// interned property-name string — while `next_keys` is a strong root and both
/// ShapeIds are stable non-pointer metadata. A dead weak key drops the entry.
#[test]
fn test_dead_transition_cache_key_pruned_on_full_gc() {
    let _guard = CopyingNurseryTestGuard::new(1);
    gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);

    // `next_keys` is a strong root of the entry and must stay live, or the test
    // would be measuring the wrong pointer. It goes in OLD-GEN on purpose:
    // arena block reset is all-or-nothing, so a reachable neighbour in
    // `dead_key`'s own nursery block would force-MARK it (#7975,
    // `mark_block_persisting_arena_objects`) and the prune would correctly
    // decline — the test would then blame the prune for something it got right.
    let next_keys = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_ARRAY) as usize;
    js_shadow_slot_set(0, ptr_bits(next_keys));
    crate::arena::arena_reset_all_blocks_to_zero();
    let dead_key = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_STRING) as usize;
    let prev_shape_id = crate::object::shapes::shape_id_for_keys_ensure(std::ptr::null(), 0);

    let occupancy_before = crate::object::test_transition_cache_occupancy();
    crate::object::test_seed_transition_cache_entry(prev_shape_id, dead_key, next_keys);
    assert_eq!(
        crate::object::test_transition_cache_occupancy(),
        occupancy_before + 1,
        "test premise: the transition entry must be installed"
    );

    full_gc();

    assert_eq!(
        crate::object::test_transition_cache_occupancy(),
        occupancy_before,
        "a transition entry whose interned key died must be dropped, not \
         rekeyed out of the bytes that replaced it"
    );
    js_shadow_slot_set(0, 0);
}

#[test]
fn test_dead_transition_cache_predecessor_shape_pruned_on_full_gc() {
    let _guard = CopyingNurseryTestGuard::new(1);
    gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);

    let next_keys = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_ARRAY) as usize;
    js_shadow_slot_set(0, ptr_bits(next_keys));
    crate::arena::arena_reset_all_blocks_to_zero();
    let dead_predecessor_keys = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_ARRAY) as usize;
    let prev_shape_id = crate::object::shapes::shape_id_for_keys_ensure(
        dead_predecessor_keys as *const crate::ArrayHeader,
        0,
    );

    let occupancy_before = crate::object::test_transition_cache_occupancy();
    crate::object::test_seed_transition_cache_entry(prev_shape_id, 0, next_keys);
    assert_eq!(
        crate::object::test_transition_cache_occupancy(),
        occupancy_before + 1,
        "test premise: the transition entry must be installed"
    );

    full_gc();

    assert_eq!(
        crate::object::test_transition_cache_occupancy(),
        occupancy_before,
        "a transition whose predecessor descriptor died must release its rooted target"
    );
    js_shadow_slot_set(0, 0);
}

#[test]
fn test_live_transition_cache_entry_survives_full_gc() {
    // Two shadow slots: the strong target and weak lookup key both stay live.
    let _guard = CopyingNurseryTestGuard::new(2);
    gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);

    let next_keys = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_ARRAY) as usize;
    let live_key = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_STRING) as usize;
    let prev_shape_id = crate::object::shapes::shape_id_for_keys_ensure(std::ptr::null(), 0);
    js_shadow_slot_set(0, ptr_bits(next_keys));
    js_shadow_slot_set(1, ptr_bits(live_key));

    let occupancy_before = crate::object::test_transition_cache_occupancy();
    crate::object::test_seed_transition_cache_entry(prev_shape_id, live_key, next_keys);

    full_gc();

    assert_eq!(
        crate::object::test_transition_cache_occupancy(),
        occupancy_before + 1,
        "an entry whose interned key is still ROOTED must survive"
    );
    js_shadow_slot_set(0, 0);
    js_shadow_slot_set(1, 0);
}

/// #8194. `REFLECT_METADATA`'s key carries the decorator target's NaN-boxed
/// bits, rekeyed by `rewrite_metadata_target_bits`. Only `Reflect.deleteMetadata`
/// ever removed an entry, so an unreachable target left its address behind.
#[test]
fn test_dead_reflect_metadata_target_pruned_on_full_gc() {
    let _guard = GcTestIsolationGuard::new();

    let len_before = crate::proxy::test_reflect_metadata_len();
    let dead = alloc_nursery_test_object_addr();
    crate::proxy::test_seed_reflect_metadata(ptr_bits(dead), "design:type");
    assert_eq!(
        crate::proxy::test_reflect_metadata_len(),
        len_before + 1,
        "test premise: the metadata entry must be installed"
    );

    full_gc();

    assert_eq!(
        crate::proxy::test_reflect_metadata_len(),
        len_before,
        "a dead decorator target's REFLECT_METADATA entry must be pruned"
    );
}

#[test]
fn test_live_reflect_metadata_target_survives_full_gc() {
    let _guard = CopyingNurseryTestGuard::new(1);

    let len_before = crate::proxy::test_reflect_metadata_len();
    let live = alloc_nursery_test_object_addr();
    js_shadow_slot_set(0, ptr_bits(live));
    crate::proxy::test_seed_reflect_metadata(ptr_bits(live), "design:type");

    full_gc();

    assert_eq!(
        crate::proxy::test_reflect_metadata_len(),
        len_before + 1,
        "a ROOTED decorator target's metadata entry must survive"
    );
    js_shadow_slot_set(0, 0);
}

/// #8195. The accessor table shares its owner key with `SYMBOL_PROPERTIES` and
/// `SYMBOL_PROPERTY_ATTRS`, both of which `prune_dead_symbol_property_owners`
/// has pruned since the 2026-07-09 audit. It was the one left out, which also
/// made every accessor closure it held immortal.
#[test]
fn test_dead_symbol_accessor_owner_pruned_on_full_gc() {
    let _guard = GcTestIsolationGuard::new();

    let count_before = crate::symbol::test_symbol_accessor_property_count();
    let dead = alloc_nursery_test_object_addr();
    crate::symbol::test_seed_symbol_accessor_property(
        dead,
        0x8195_0001,
        crate::value::TAG_UNDEFINED,
    );
    assert_eq!(
        crate::symbol::test_symbol_accessor_property_count(),
        count_before + 1,
        "test premise: the accessor descriptor must be installed"
    );

    full_gc();

    assert_eq!(
        crate::symbol::test_symbol_accessor_property_count(),
        count_before,
        "a dead owner's SYMBOL_ACCESSOR_PROPERTIES entry must be pruned"
    );
}

#[test]
fn test_live_symbol_accessor_owner_survives_full_gc() {
    let _guard = CopyingNurseryTestGuard::new(1);

    let count_before = crate::symbol::test_symbol_accessor_property_count();
    let live = alloc_nursery_test_object_addr();
    js_shadow_slot_set(0, ptr_bits(live));
    crate::symbol::test_seed_symbol_accessor_property(
        live,
        0x8195_0002,
        crate::value::TAG_UNDEFINED,
    );

    full_gc();

    assert_eq!(
        crate::symbol::test_symbol_accessor_property_count(),
        count_before + 1,
        "a ROOTED owner's accessor descriptor must survive"
    );
    js_shadow_slot_set(0, 0);
}

// --- per-object layout tables (LAYOUT_SLOT_MASKS + TYPED_LAYOUTS) -----------
//
// The inline allocator's forget probe is gated on
// `PERRY_YOUNG_LAYOUT_RECORDS`: the count of records keyed by an address the
// nursery could hand out again. These pin the two halves of that contract —
// a dead nursery owner's record is pruned by the copied-minor pass (so it can
// never be inherited), and the count returns to zero once no nursery-keyed
// record remains, while a live old-page record keeps the armed flag without
// re-opening the probe.

fn young_layout_records() -> u32 {
    crate::gc::layout_tables::test_young_layout_records()
}

unsafe fn install_typed_record(addr: usize) {
    let pointer_mask = [0b10u64];
    crate::gc::js_gc_init_typed_shape_layout(
        addr as u64,
        2,
        std::ptr::null(),
        0,
        pointer_mask.as_ptr(),
        pointer_mask.len() as u32,
    );
}

#[test]
fn test_dead_nursery_owner_layout_record_pruned_on_copied_minor_and_young_count_drops() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let before = young_layout_records();
    let (obj, _) = unsafe { alloc_nursery_test_object(2) };
    let addr = obj as usize;
    unsafe { install_typed_record(addr) };
    assert!(
        crate::gc::layout_tables::test_per_object_layout_present(addr),
        "premise: the nursery owner carries a typed record"
    );
    assert!(
        young_layout_records() > before,
        "a fresh nursery-keyed record must count as young until a collection proves otherwise"
    );
    js_shadow_slot_set(0, 0);

    let _ = gc_collect_minor();

    assert!(
        !crate::gc::layout_tables::test_per_object_layout_present(addr),
        "dead from-space owner's per-object record must be pruned by the copied-minor pass"
    );
    assert_eq!(
        young_layout_records(),
        before,
        "after the prune no nursery-keyed record remains, so the inline allocator's gate must read zero"
    );
}

#[test]
fn test_live_old_owner_layout_record_keeps_flag_armed_but_not_young() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let before = young_layout_records();
    let addr = unsafe {
        let shape_id = crate::object::shapes::shape_descriptor_ensure(std::ptr::null(), 0, 2)
            .expect("shape id range exhausted in a test fixture");
        let obj = gc_malloc(
            std::mem::size_of::<crate::object::ObjectHeader>() + 2 * 8,
            GC_TYPE_OBJECT,
        ) as *mut crate::object::ObjectHeader;
        (*obj).class_id = 0;
        (*obj).parent_class_id = shape_id;
        (*obj).meta = std::ptr::null_mut();
        let fields =
            (obj as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *mut u64;
        *fields = 0;
        *fields.add(1) = 0;
        obj as usize
    };
    unsafe { install_typed_record(addr) };
    assert!(
        crate::gc::layout_tables::test_per_object_layout_present(addr),
        "premise: the malloc'd owner carries a typed record"
    );
    assert_ne!(
        crate::gc::layout_tables::test_per_object_layout_armed_threads(),
        0,
        "a live record keeps the armed-thread count non-zero"
    );
    assert_eq!(
        young_layout_records(),
        before,
        "a record on a gc_malloc page is not one the bump allocator can recycle"
    );
    crate::gc::layout_clear_for_ptr(addr);
}
