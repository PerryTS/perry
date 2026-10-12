//! Temporal owns its expando bag through a traced metadata edge.
use super::*;

#[cfg(feature = "temporal")]
#[test]
fn temporal_expando_bag_moves_with_its_native_owner() {
    check_objtables_temporal(false);
}

#[cfg(feature = "temporal")]
#[test]
#[ignore = "negative control: intentionally erase the owner invariant"]
fn negative_control_objtables_temporal() {
    check_objtables_temporal(true);
}

#[cfg(feature = "temporal")]
fn check_objtables_temporal(erase_invariant: bool) {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    let date =
        temporal_rs::PlainDate::try_new(2026, 10, 11, temporal_rs::Calendar::default()).unwrap();
    let owner =
        crate::temporal::alloc_temporal_cell(crate::temporal::TemporalValue::PlainDate(date));
    let addr = (owner.to_bits() & POINTER_MASK) as usize;
    let text = crate::string::js_string_from_bytes(b"temporal-child".as_ptr(), 14);
    crate::object::exotic_expando::test_define_exotic_property(
        addr,
        "tag",
        crate::value::JSValue::string_ptr(text).bits(),
    );
    js_shadow_slot_set(0, owner.to_bits());
    let cycles = copying_minor_cycles();
    gc_collect_minor();
    assert!(copying_minor_cycles() > cycles);
    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(addr, moved, "the Temporal cell must move");
    assert!(crate::temporal::is_temporal_cell_addr(moved));
    if erase_invariant {
        unsafe {
            (*(moved as *mut crate::temporal::TemporalCell)).meta = std::ptr::null_mut();
        }
    }
    let bag = unsafe { crate::object::cell_expando_get(moved).unwrap() };
    let key = crate::string::js_string_from_bytes(b"tag".as_ptr(), 3);
    let child = crate::object::js_object_get_field_by_name(bag, key);
    let string = crate::value::js_get_string_pointer_unified(f64::from_bits(child.bits()))
        as *const crate::string::StringHeader;
    assert_ne!(string, text.cast_const(), "the bag's child must move too");
    assert_eq!(
        unsafe {
            std::slice::from_raw_parts(
                crate::string::string_data(string),
                (*string).byte_len as usize,
            )
        },
        b"temporal-child"
    );
    // Negative control: severing the owner edge removes its property storage.
    unsafe {
        (*(moved as *mut crate::temporal::TemporalCell)).meta = std::ptr::null_mut();
    }
    assert!(unsafe { crate::object::cell_expando_get(moved).is_none() });
    js_shadow_slot_set(0, 0);
}
