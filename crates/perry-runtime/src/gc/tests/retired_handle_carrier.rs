//! A carried shape record's owned handle may name a record an earlier full
//! trace retired. `recompute_record_handle_carriers` followed such a word to
//! the shared read-only `EMPTY_RECORD` and wrote its carrier bit: a SIGSEGV
//! in the second full collection (found by WRT1's image fixture, reproduced
//! on main b8c7a9770 with an ordinary heap object).
use super::support::*;
use crate::gc::*;
use crate::object::ObjectHeader;

fn key(name: &str) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

#[test]
fn a_full_after_a_full_survives_retired_owned_handles() {
    let _copying = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    register_runtime_handle_root_scanner_for_tests();
    crate::test_support::bootstrap_test_thread();
    let desc = std::ptr::addr_of!(crate::object::runtime_layouts::ITERATOR_RESULT);
    let obj = unsafe { crate::object::layout_desc::perry_alloc_layout(desc) };
    let scope = RuntimeHandleScope::new();
    let rooted = scope.root_raw_mut_ptr(obj as *mut u8);
    // Growing a layout object past its slots walks a transition chain whose
    // intermediate records the first full retires.
    for i in 0..24 {
        let value = format!("value-{i}");
        let s = crate::string::js_string_from_bytes(value.as_ptr(), value.len() as u32);
        crate::object::js_object_set_field_by_name(
            rooted.get_raw_mut_ptr::<ObjectHeader>(),
            key(&format!("k{i}")),
            f64::from_bits(string_bits(s as usize)),
        );
    }
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    let old_in_use = crate::arena::old_gen_in_use_bytes();
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.set(old_in_use));
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(true));
    let mut result = JsGcStepResult::default();
    assert_eq!(
        js_gc_step_work_units(1, &mut result),
        JS_GC_STEP_STATUS_ACTIVE
    );
    complete_budgeted_gc_cycle();
    for i in 0..24 {
        let bits = crate::object::js_object_get_field_by_name(
            rooted.get_raw_mut_ptr::<ObjectHeader>(),
            key(&format!("k{i}")),
        )
        .bits();
        unsafe {
            assert_string_bytes(
                (bits & POINTER_MASK) as *const crate::StringHeader,
                format!("value-{i}").as_bytes(),
            )
        };
    }
}
