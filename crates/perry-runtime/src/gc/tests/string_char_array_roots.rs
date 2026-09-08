//! Moving-GC regression for the string-spread character array builder (#9983).

use super::super::*;
use super::support::*;

thread_local! {
    static COPIED_OBJECTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn force_minor_before_character_allocation() {
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    COPIED_OBJECTS.with(|count| {
        count.set(count.get() + trace.copying_nursery.copied_objects);
    });
}

#[test]
fn string_character_array_rereads_result_after_each_moving_minor() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner_with_source(
        scan_runtime_handle_roots_mut,
        MutableRootScannerSource::RuntimeHandles,
    );
    COPIED_OBJECTS.with(|count| count.set(0));

    let text = "ééééééééééééé";
    let source = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    let result = crate::string::test_string_to_char_array_with_pre_element_alloc_hook(
        (crate::value::STRING_TAG | source as u64) as i64,
        force_minor_before_character_allocation,
    ) as *mut crate::array::ArrayHeader;

    assert!(
        COPIED_OBJECTS.with(|count| count.get()) > 0,
        "the forced collections moved nothing, so the result assertions are vacuous"
    );
    assert_eq!(unsafe { (*result).length }, 13);
    for index in 0..13 {
        let value = crate::array::js_array_get_f64(result, index);
        let ptr = (value.to_bits() & POINTER_MASK) as *const crate::StringHeader;
        unsafe { assert_string_bytes(ptr, "é".as_bytes()) };
    }

    let header = unsafe { header_from_user_ptr(result as *const u8) };
    let mut stats = ArraySlotEnumerationStats::default();
    unsafe { verify_array_pointer_slots_enumerated_for(&mut stats, header) };
    assert_eq!(stats.unenumerated_slots, 0);
    assert_eq!(stats.checked_pointer_slots, 13);
}
