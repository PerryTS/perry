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

    // OBJECT_PROTOTYPES: recorded `Object.setPrototypeOf(obj, null)`.
    let (proto_owner, _) = unsafe { alloc_nursery_test_object(0) };
    let proto_owner = proto_owner as usize;
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
