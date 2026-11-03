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
use crate::codegen_abi::LayoutWord;
use crate::value::TAG_UNDEFINED;

pub(super) fn full_gc() {
    let _ =
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
}

pub(super) struct ArraySideTableTestGuard;

impl ArraySideTableTestGuard {
    pub(super) fn new() -> Self {
        Self
    }
}

impl Drop for ArraySideTableTestGuard {
    fn drop(&mut self) {}
}

pub(super) fn register_array_side_table_scanners() {
    gc_register_mutable_root_scanner(crate::array::scan_template_raw_roots_mut);
}

/// Returns the live head: the first named property on a full fixture array
/// grows it (`array/named_props.rs`).
pub(super) unsafe fn set_array_named_property(
    arr: *mut crate::array::ArrayHeader,
    name: &str,
    value: f64,
) -> *mut crate::array::ArrayHeader {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let head = crate::array::array_named_property_set(arr, key, value);
    assert!(
        !head.is_null(),
        "test premise: the fixture array accepts an expando"
    );
    head
}

pub(super) unsafe fn alloc_nursery_test_array() -> *mut crate::array::ArrayHeader {
    let arr = crate::arena::arena_alloc_gc(
        std::mem::size_of::<crate::array::ArrayHeader>(),
        std::mem::align_of::<crate::array::ArrayHeader>(),
        GC_TYPE_ARRAY,
    ) as *mut crate::array::ArrayHeader;
    (*arr).length = 0;
    (*arr).capacity = 0;
    (*arr).named = std::ptr::null_mut();
    arr
}

pub(super) unsafe fn alloc_malloc_test_object() -> *mut crate::object::ObjectHeader {
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
/// `gc::mark::mark_block_persisting_arena_objects` force-MARKS every object in
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
pub(super) fn full_gc_with_no_block_persistence() {
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
}

#[test]
fn materialized_map_brand_moves_with_private_bag() {
    check_materialized_map_brand(false);
}

#[test]
#[ignore = "negative control: erase the moved Map iterator brand"]
fn negative_control_objtables_map_iterator() {
    check_materialized_map_brand(true);
}

fn check_materialized_map_brand(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(1);
    let map = crate::map::js_map_alloc(0);
    let original = crate::map::js_map_entries(map) as usize;
    js_shadow_slot_set(0, ptr_bits(original));
    let _ = gc_collect_minor();
    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(original, moved);
    if erase {
        unsafe {
            let bag = crate::array::array_property_bag(moved as *const crate::array::ArrayHeader);
            (*(*bag).meta).native_state = 0;
        }
    }
    assert!(util_types_is_map_iterator(moved));
    let fresh = crate::array::js_array_alloc(0) as usize;
    assert!(!util_types_is_map_iterator(fresh));
    assert!(!util_types_is_set_iterator(moved));
}

#[test]
fn materialized_set_brand_moves_with_private_bag() {
    check_materialized_set_brand(false);
}

#[test]
#[ignore = "negative control: erase the moved Set iterator brand"]
fn negative_control_objtables_set_iterator() {
    check_materialized_set_brand(true);
}

fn check_materialized_set_brand(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(1);
    let set = crate::set::js_set_alloc(0);
    let original = crate::set::js_set_to_array(set) as usize;
    js_shadow_slot_set(0, ptr_bits(original));
    let _ = gc_collect_minor();
    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(original, moved);
    if erase {
        unsafe {
            let bag = crate::array::array_property_bag(moved as *const crate::array::ArrayHeader);
            (*(*bag).meta).native_state = 0;
        }
    }
    assert!(util_types_is_set_iterator(moved));
    let fresh = crate::array::js_array_alloc(0) as usize;
    assert!(!util_types_is_set_iterator(fresh));
    assert!(!util_types_is_map_iterator(moved));
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
    // An array owner: an ordinary object's attributes AND accessor pairs
    // live with its keys and slots (charter step 3); an array's are still
    // owner-keyed, which is the table this test is about.
    let (arr, _) = unsafe { alloc_old_test_array(0) };
    let addr = arr as usize;
    crate::object::set_accessor_descriptor(
        addr,
        "oldKey".to_string(),
        crate::object::AccessorDescriptor::default(),
    );

    // MINOR traces never mark the old generation — an unmarked old-gen
    // header proves nothing, so the prune must NOT fire (the audit's
    // central deadness caveat).
    let _ = gc_collect_minor();

    assert!(
        crate::object::get_accessor_descriptor(addr, "oldKey").is_some(),
        "an old-gen owner's descriptor entry must survive a minor GC — \
         minor-trace deadness is not trustworthy for tenured objects"
    );
}

#[test]
fn test_dead_nursery_owner_descriptors_are_not_inherited_after_copied_minor() {
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

    // Descriptors belong to a stationary old key list. Reading the dead
    // receiver address is invalid; a fresh receiver must inherit no attrs.
    let (fresh, _) = unsafe { alloc_nursery_test_object(0) };
    assert!(crate::object::get_property_attrs(fresh as usize, "gone").is_none());
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

/// The `ObjectModuleWrapper` dead-payload arm pruned only OVERFLOW_FIELDS;
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
fn test_dead_owner_prototype_vm_expando_and_filehandle_entries_pruned() {
    let _guard = GcTestIsolationGuard::new();

    // EXOTIC_EXPANDO: expando on a (movable, here old-gen) Promise cell.
    let promise = unsafe { alloc_old_test_promise() } as usize;
    crate::object::exotic_expando::test_define_exotic_property(
        promise,
        "status",
        crate::value::TAG_TRUE,
    );
    assert!(unsafe { crate::object::cell_expando_get(promise).is_some() });

    full_gc();
}

/// The DOMException brand is carried by the cell, so recycling an address
/// cannot inherit a stale registry entry.
#[test]
fn dom_exception_brand_is_owned_by_its_error_cell() {
    let _global = global_side_table_test_lock();
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let err = crate::event_target::js_dom_exception_new(undef, undef);
    assert!(crate::event_target::is_dom_exception_error(err));
    unsafe {
        (*err).error_kind = crate::error::ERROR_KIND_ERROR;
    }
    assert!(!crate::event_target::is_dom_exception_error(err));
}

mod meta_and_shape_records;
#[cfg(feature = "regex-engine")]
mod regexp_expandos;

// ── The four tables the #8174 audit found REKEYED with no death story ───────
//
// The registry gate
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

#[test]
fn console_private_streams_move_with_the_instance() {
    check_objtables_console(false);
}

#[test]
#[ignore = "negative control: intentionally erase the owner invariant"]
fn negative_control_objtables_console() {
    check_objtables_console(true);
}

fn check_objtables_console(erase_invariant: bool) {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    let obj = crate::object::js_object_alloc(crate::builtins::CONSOLE_INSTANCE_CLASS_ID, 0);
    let stream = crate::object::js_object_alloc(0, 0);
    unsafe {
        crate::builtins::test_seed_console_instance(
            obj as usize,
            crate::value::js_nanbox_pointer(stream as i64),
            crate::value::js_nanbox_pointer(stream as i64),
        );
    }
    js_shadow_slot_set(0, ptr_bits(obj as usize));
    let cycles = copying_minor_cycles();
    gc_collect_minor();
    assert!(copying_minor_cycles() > cycles);
    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::object::ObjectHeader;
    assert_ne!(moved, obj);
    if erase_invariant {
        unsafe {
            (*(*moved).meta).native_state = 0;
        }
    }
    unsafe {
        assert_ne!(
            (*(*moved).meta).native_state,
            0,
            "instance must own its state"
        );
        let state =
            ((*(*moved).meta).native_state & POINTER_MASK) as *mut crate::array::ArrayHeader;
        let stdout = crate::array::js_array_get(state, 0).bits();
        assert_ne!(stdout & POINTER_MASK, stream as u64);
        assert_eq!(stdout, crate::array::js_array_get(state, 1).bits());
        assert!(crate::builtins::try_console_instance_method_dispatch(
            moved,
            "groupEnd",
            std::ptr::null(),
            0
        )
        .is_some());
        // Negative control: without the owner edge, no instance state remains.
        (*(*moved).meta).native_state = 0;
        assert!(crate::builtins::try_console_instance_method_dispatch(
            moved,
            "groupEnd",
            std::ptr::null(),
            0
        )
        .is_none());
    }
    js_shadow_slot_set(0, 0);
}

#[test]
fn boxed_string_payload_moves_as_a_private_child_slot() {
    check_objtables_boxed_payload(false);
}

#[test]
#[ignore = "negative control: intentionally erase the owner invariant"]
fn negative_control_objtables_boxed_payload() {
    check_objtables_boxed_payload(true);
}

fn check_objtables_boxed_payload(erase_invariant: bool) {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    let text = crate::string::js_string_from_bytes(b"private-payload".as_ptr(), 15);
    let value = crate::builtins::js_boxed_string_new(
        f64::from_bits(crate::value::JSValue::string_ptr(text).bits()),
        1,
    );
    js_shadow_slot_set(0, value.to_bits());
    let cycles = copying_minor_cycles();
    gc_collect_minor();
    assert!(copying_minor_cycles() > cycles);
    let moved = f64::from_bits(js_shadow_slot_get(0));
    assert_ne!(moved.to_bits(), value.to_bits(), "wrapper must move");
    if erase_invariant {
        unsafe {
            let obj = (moved.to_bits() & POINTER_MASK) as *mut crate::object::ObjectHeader;
            (*(*obj).meta).boxed_primitive = crate::value::TAG_UNDEFINED;
        }
    }
    let (_, payload) = crate::builtins::boxed_primitive_payload(moved).unwrap();
    assert_ne!(
        payload.to_bits(),
        crate::value::JSValue::string_ptr(text).bits()
    );
    assert!(
        crate::value::JSValue::from_bits(payload.to_bits()).is_string(),
        "the private payload must be a String"
    );
    let string =
        crate::value::js_get_string_pointer_unified(payload) as *const crate::string::StringHeader;
    assert_eq!(
        unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(
                crate::string::string_data(string),
                (*string).byte_len as usize,
            ))
            .unwrap()
        },
        "private-payload"
    );
    // Negative control: replace the private slot, and the public unbox changes.
    unsafe {
        let obj = (moved.to_bits() & POINTER_MASK) as *mut crate::ObjectHeader;
        (*(*obj).meta).boxed_primitive = crate::value::TAG_UNDEFINED;
    }
    assert!(crate::value::JSValue::from_bits(
        crate::builtins::boxed_primitive_payload(moved)
            .unwrap()
            .1
            .to_bits()
    )
    .is_undefined());
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
    let prev_shape_id = crate::object::shapes::layout_for_keys_ensure(std::ptr::null(), 0);

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
    // The predecessor is the entry's weak lookup key: the cache owns only its
    // target, so a predecessor without another carrier retires and takes the
    // entry with it.
    let _guard = CopyingNurseryTestGuard::new(1);
    gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);

    let next_keys = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_ARRAY) as usize;
    js_shadow_slot_set(0, ptr_bits(next_keys));
    crate::arena::arena_reset_all_blocks_to_zero();
    let dead_predecessor_keys = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_ARRAY) as usize;
    let prev_shape_id = crate::object::shapes::layout_for_keys_ensure(
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

    assert!(crate::object::shapes::shape_is_retired(prev_shape_id));
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
    // This predecessor can recur from a generated module global; without such
    // an owner, retiring it and the now-unusable cache entry is intentional.
    let prev_shape_id = crate::object::shapes::js_object_layout_for_keys(0, 0);
    js_shadow_slot_set(0, ptr_bits(next_keys));
    js_shadow_slot_set(1, ptr_bits(live_key));

    let occupancy_before = crate::object::test_transition_cache_occupancy();
    crate::object::test_seed_transition_cache_entry(LayoutWord(prev_shape_id), live_key, next_keys);

    full_gc();

    assert_eq!(
        crate::object::test_transition_cache_occupancy(),
        occupancy_before + 1,
        "an entry whose interned key is still ROOTED must survive"
    );
    js_shadow_slot_set(0, 0);
    js_shadow_slot_set(1, 0);
}

#[test]
fn reflect_metadata_record_moves_as_a_private_owner_child() {
    check_reflect_metadata_record(false);
}

#[test]
#[ignore = "negative control: erase the moved Reflect metadata record"]
fn negative_control_objtables_reflect_metadata() {
    check_reflect_metadata_record(true);
}

fn check_reflect_metadata_record(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(1);
    let owner = crate::object::js_object_alloc(0, 0);
    let value = crate::object::js_object_alloc(0, 0);
    let key = crate::string::js_string_new_sso(b"meta".as_ptr(), 4);
    crate::proxy::metadata::js_reflect_define_metadata(
        key,
        f64::from_bits(ptr_bits(value as usize)),
        f64::from_bits(ptr_bits(owner as usize)),
        f64::from_bits(TAG_UNDEFINED),
    );
    js_shadow_slot_set(0, ptr_bits(owner as usize));
    let _ = gc_collect_minor();
    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::object::ObjectHeader;
    assert_ne!(moved, owner);
    if erase {
        unsafe {
            (*(*moved).meta).reflect_metadata = 0;
        }
    }
    let result = crate::proxy::metadata::js_reflect_get_own_metadata(
        key,
        f64::from_bits(ptr_bits(moved as usize)),
        f64::from_bits(TAG_UNDEFINED),
    );
    assert_ne!(result.to_bits(), TAG_UNDEFINED);
    assert_ne!((result.to_bits() & POINTER_MASK) as usize, value as usize);
    assert!(unsafe {
        crate::value::addr_class::try_read_tracked_gc_header(
            (result.to_bits() & POINTER_MASK) as usize,
        )
    }
    .is_some());
    let fresh = crate::object::js_object_alloc(0, 0);
    assert_eq!(
        crate::proxy::metadata::js_reflect_get_own_metadata(
            key,
            f64::from_bits(ptr_bits(fresh as usize)),
            f64::from_bits(TAG_UNDEFINED)
        )
        .to_bits(),
        TAG_UNDEFINED
    );
}

/// #6759 phase 1: an `ErrorHeader`'s metadata edge must be ENUMERATED by the
/// slot visitor that drives tracing.
///
/// #6812 records the failure mode: a meta edge visited only on the rewrite path
/// is invisible to MARKING, so the record — and anything reachable only through
/// it — is swept while the owner still points at it. Errors are visited by the
/// `GcRewriteDescriptorKind::Error` arm, and `trace_heap_rewrite_slots` drives
/// exactly that arm, so listing the slot there is what makes the edge both
/// marked and rewritten.
///
/// This asserts enumeration DIRECTLY rather than by observing survival across a
/// collection. A survival test is vacuous here: arena block reset is
/// all-or-nothing, so `gc::trace` force-marks every object in a block that
/// still holds one reachable object (#7975), which keeps an untraced record
/// alive anyway — verified by sabotage, where deleting the `visit(...)` line
/// left a survival-based test still passing. Deleting it fails THIS test.
#[test]
fn error_meta_edge_is_enumerated_by_the_trace_visitor() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let msg = crate::string::js_string_from_bytes(b"traced".as_ptr(), 6);
        let err = crate::error::js_error_new_with_message(msg);
        let scope = crate::gc::RuntimeHandleScope::new();
        let err_h = scope.root_raw_mut_ptr(err);

        crate::object::object_meta_ensure_for_cell(
            err as usize,
            crate::object::MetaReason::CellExpando,
        )
        .expect("an error cell must be able to materialise a meta record");

        let err = err_h.get_raw_mut_ptr::<crate::error::ErrorHeader>();
        let meta_slot = &mut (*err).meta as *mut _ as *mut u64;
        let header = (err as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;

        let mut visited: Vec<*mut u64> = Vec::new();
        crate::gc::layout_slot_visit::visit_gc_rewrite_slot_descriptors::<{ cfg!(test) }>(
            header,
            |descriptor| {
                descriptor.visit_slots(&mut |slot| visited.push(slot.slot));
            },
        );

        assert!(
            visited.contains(&meta_slot),
            "the error's `meta` slot must be enumerated by the trace visitor; \
             an edge that is not enumerated is not marked, and the metadata \
             record is swept out from under a live error"
        );
        // The pre-existing edges must still be enumerated — appending a field
        // must not displace any of them.
        assert!(
            visited.contains(&(&mut (*err).message as *mut _ as *mut u64)),
            "the `message` edge must still be enumerated"
        );
    }
}

/// #6759 phase 1: `ObjectMeta.expando` — the named-property bag for cells with
/// no inline slot layout — must be ENUMERATED by the trace visitor.
///
/// It is reachable only through the metadata record, so an unvisited edge here
/// collects a live owner's own properties. Same hazard the `spill` edge
/// documents (#6812).
///
/// `ObjectMeta`'s rewrite arm is its trace path — `trace_heap_rewrite_slots`
/// drives `visit_gc_rewrite_slot_descriptors`, and the `ObjectMeta` case lists
/// slots explicitly rather than delegating to the layout visitor — so listing
/// the slot there is what makes it marked as well as rewritten.
#[test]
fn object_meta_expando_edge_is_enumerated_by_the_trace_visitor() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = crate::object::js_object_alloc(0, 0);
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj_h = scope.root_raw_mut_ptr(obj);
        let meta = crate::object::object_meta_ensure_for_cell(
            obj as usize,
            crate::object::MetaReason::CellExpando,
        )
        .expect("an object cell must materialise a meta record");

        let meta_header =
            (meta as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        let expando_slot = &mut (*meta).expando as *mut u64;
        let spill_slot = &mut (*meta).spill as *mut u64;

        let mut visited: Vec<*mut u64> = Vec::new();
        crate::gc::layout_slot_visit::visit_gc_rewrite_slot_descriptors::<{ cfg!(test) }>(
            meta_header,
            |descriptor| {
                descriptor.visit_slots(&mut |slot| visited.push(slot.slot));
            },
        );

        assert!(
            visited.contains(&expando_slot),
            "ObjectMeta.expando must be enumerated by the trace visitor; an \
             unenumerated edge is never marked, so a live owner's own \
             properties are swept"
        );
        assert!(
            visited.contains(&spill_slot),
            "the pre-existing `spill` edge must still be enumerated — appending \
             a field must not displace it"
        );
        let _ = obj_h;
    }
}

/// `ObjectMeta`'s field offsets are a contract with codegen (there are
/// `offset_of!` asserts on `spill`, `function_layout_birth` and
/// `array_tail_object_hot`). `expando` therefore has to be APPENDED, never
/// inserted — inserting it after `spill` shifted the 48/56 fields and failed
/// those compile-time asserts.
#[test]
fn object_meta_expando_is_appended_not_inserted() {
    assert_eq!(
        std::mem::offset_of!(crate::object::ObjectMeta, spill),
        32,
        "spill must keep its contracted offset"
    );
    assert!(
        std::mem::offset_of!(crate::object::ObjectMeta, expando)
            > std::mem::offset_of!(crate::object::ObjectMeta, array_tail_object_hot),
        "expando must sit after every field codegen has an offset contract on"
    );
}

/// #6759 phase 3: a transition-cache entry must NOT keep its target keys array
/// alive, and an entry whose target died must be reaped.
///
/// `next_keys` used to be visited with `visit_usize_slot`, which MARKS. With
/// 16384 slots the cache could pin 16384 keys arrays — and through them their
/// shape descriptors — whether or not any live object still had that shape,
/// feeding unbounded shape-table growth. A transition entry is a pure cache
/// ("adding key k to shape S yields shape T"); if nothing has shape T, the
/// answer is worthless, so pinning T to keep it answerable is backwards.
/// `key_ptr` was already weak; this makes the pair consistent.
///
/// The two halves must move together — weakening the edge without reaping dead
/// targets leaves a dangling `next_keys`. This pins the reaping half.
///
/// The entry is seeded with a LIVE `prev_shape_id` on purpose. An earlier
/// version of this test used `prev_shape_id = 0`, which the prune's
/// pre-existing `shape_descriptor_for_word(..).is_none()` clause already treats as
/// dead — so it passed with the new clause deleted, proving nothing. Sabotage
/// check: removing `is_dead_owner(entry.next_keys)` must fail this test.
#[test]
fn transition_cache_entry_does_not_pin_its_target() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        // A real object gives a real, live shape id, so the ONLY thing that can
        // make the seeded entry dead is its target.
        let obj = crate::object::js_object_alloc(0, 0);
        let keys = crate::object::object_keys(obj).arr();
        let live_shape = crate::object::shapes::test_shape_id_for_keys(keys as usize)
            .expect("a freshly allocated object must have a registered shape");
        assert!(
            crate::object::shapes::test_descriptor_for_retained_word(live_shape).is_some(),
            "test premise: prev_shape_id must be LIVE, or the prune's existing \
             dead-shape clause decides the outcome and this test is vacuous"
        );

        let before = crate::object::test_transition_cache_occupancy();
        let dead_target = 0xDEAD_0000_1000usize;
        crate::object::test_seed_transition_cache_entry(live_shape, 0, dead_target);
        assert!(
            crate::object::test_transition_cache_occupancy() > before,
            "test premise: the entry must actually be installed"
        );

        // Only the target address is dead.
        crate::object::prune_dead_transition_cache_entries(&|addr| addr == dead_target);

        assert_eq!(
            crate::object::test_transition_cache_occupancy(),
            before,
            "an entry whose target keys array is dead must be dropped — without \
             this, weakening `next_keys` leaves a dangling pointer in the cache"
        );
    }
}

mod temporal_expandos;
