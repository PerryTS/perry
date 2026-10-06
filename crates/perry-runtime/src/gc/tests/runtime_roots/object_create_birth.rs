//! Final-shape births retain and refresh their prototype across allocation.
use super::*;

struct RelocationGuard(Option<crate::gc::roots::ConservativeStackScanMode>);
impl RelocationGuard {
    fn new() -> Self {
        Self(crate::gc::roots::set_conservative_stack_scan_override(
            Some(crate::gc::roots::ConservativeStackScanMode::Disabled),
        ))
    }
}
impl Drop for RelocationGuard {
    fn drop(&mut self) {
        crate::gc::roots::set_conservative_stack_scan_override(self.0);
    }
}

#[test]
fn object_create_refreshes_its_prototype_when_birth_allocation_moves_it() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _age = crate::gc::tenuring::set_survivals_for_test(4);
    let _pacing = crate::gc::policy::force_alloc_point_minor_pacing();
    let _relocation = RelocationGuard::new();
    let triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_prototype_words_mut);
    let proto = crate::object::js_object_alloc(0, 0);
    // Pre-mark so the injected collection lands in the newborn allocation,
    // rather than in the prototype's first meta-record allocation.
    unsafe { crate::object::proto_validity::mark_object_as_prototype(proto as usize) };
    let before = proto as usize;
    js_shadow_slot_set(0, ptr_bits(before));
    force_next_general_arena_alloc_slow();
    triggers.make_arena_trigger_due();
    let born = crate::object::js_object_create(f64::from_bits(ptr_bits(before)));
    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(moved, before, "the prototype must move inside the birth");
    let obj = crate::value::js_nanbox_get_pointer(born) as *mut crate::ObjectHeader;
    assert!(
        unsafe { (*obj).meta }.is_null(),
        "no per-instance prototype record"
    );
    assert_eq!(
        crate::object::prototype_chain::object_static_prototype(obj as usize),
        Some(ptr_bits(moved))
    );
    assert_eq!(
        crate::object::shapes::shape_object_kind_by_id(unsafe {
            crate::object::shapes::object_shape_stamp(obj)
        }),
        Some(crate::object::shapes::ShapeObjectKind::Ordinary)
    );
    // Retain only the newborn: the next copying minor must trace its shape's
    // prototype edge and rewrite both addresses, without the caller's root.
    js_shadow_slot_set(0, born.to_bits());
    let _ = gc_collect_minor();
    let owner = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(owner, obj as usize, "the newborn must move");
    let linked = crate::object::prototype_chain::object_static_prototype(owner).unwrap();
    assert_ne!(linked, ptr_bits(moved), "the prototype must move again");
    assert!(crate::object::is_valid_obj_ptr(
        (linked & POINTER_MASK) as *const u8
    ));
}

#[test]
fn object_create_retains_its_descriptor_bag_across_birth_collection() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _pacing = crate::gc::policy::force_alloc_point_minor_pacing();
    let _relocation = RelocationGuard::new();
    let triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_prototype_words_mut);
    gc_register_mutable_root_scanner(crate::object::canonical_keys::scan_canonical_keys_roots_mut);
    gc_register_mutable_root_scanner(crate::string::scan_intern_table_roots_mut);
    let proto = crate::object::js_object_alloc(0, 0);
    unsafe { crate::object::proto_validity::mark_object_as_prototype(proto as usize) };
    js_shadow_slot_set(0, ptr_bits(proto as usize));
    // Null-prototype bags avoid lazy realm bootstrap in the injected window.
    let desc = crate::object::js_object_alloc_null_proto(0, 0);
    let value_key = crate::string::canonical_key(b"value");
    crate::object::js_object_set_field_by_name(desc, value_key, 42.0);
    let bag = crate::object::js_object_alloc_null_proto(0, 0);
    let x_key = crate::string::canonical_key(b"x");
    crate::object::js_object_set_field_by_name(bag, x_key, f64::from_bits(ptr_bits(desc as usize)));
    force_next_general_arena_alloc_slow();
    triggers.make_arena_trigger_due();
    // No caller root holds the bag: its lifetime belongs to the adapter.
    let result = crate::object::js_object_create_with_props(
        f64::from_bits(ptr_bits(proto as usize)),
        f64::from_bits(ptr_bits(bag as usize)),
    );
    assert_ne!(
        js_shadow_slot_get(0),
        ptr_bits(proto as usize),
        "a copying collection must land inside the birth"
    );
    let scope = RuntimeHandleScope::new();
    let result = scope.root_nanbox_f64(result);
    let key = crate::string::canonical_key(b"x");
    let owner =
        crate::value::js_nanbox_get_pointer(result.get_nanbox_f64()) as *const crate::ObjectHeader;
    assert_eq!(
        crate::object::js_object_get_field_by_name(owner, key).bits(),
        42.0f64.to_bits()
    );
}
