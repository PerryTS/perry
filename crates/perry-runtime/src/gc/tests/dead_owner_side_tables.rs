//! Death pruning for object-address-keyed side tables (2026-07-09 GC audit,
//! wave 2 batch B — `gc/dead_owner.rs`).
//!
//! Each family test follows the same shape: create an owner, install a side
//! table entry keyed by its address, drop every reference, run a collection,
//! and assert the entry is gone — plus the two safety inverses (a LIVE owner's
//! entries survive; a TENURED owner's entries survive a MINOR trace, which
//! never marks the old generation and therefore proves nothing about it).

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
    (*obj).object_type = 1;
    (*obj).class_id = 0;
    (*obj).parent_class_id = 0;
    (*obj).field_count = 0;
    (*obj).keys_array = std::ptr::null_mut();
    (*obj).meta = std::ptr::null_mut();
    obj
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
    let _guard = GcTestIsolationGuard::new();
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
    full_gc();

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
}

#[test]
fn test_dead_arguments_object_entry_pruned_on_full_gc() {
    let _guard = GcTestIsolationGuard::new();
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let obj = crate::object::js_arguments_object_alloc(undefined, undefined, 0);
    let addr = obj as usize;
    assert!(crate::object::test_arguments_object_registered(addr));

    full_gc();

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

/// #6759 Phase C3a: an owned keys array's grow-realloc migrates the shape
/// record (slot map + stable shape_id) to the new address instead of
/// orphaning it.
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
        Some(id),
        "the record — including its stable shape_id — must move to the new address"
    );
    // Cleanup so the seeded address can't leak into later tests.
    crate::object::shapes::shape_drop(new_addr as *const crate::array::ArrayHeader);
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
    js_shadow_slot_set(0, 0);
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
