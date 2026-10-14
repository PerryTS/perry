//! `ProducerChildBirth`: a producer whose fresh result is born old allocates
//! the children it stores there old too, and those stores remember nothing.
//! `String.prototype.split` is the producer under test.

use super::super::*;
use super::support::*;

/// Parts whose result array crosses the pointer-bearing born-old threshold
/// (`LARGE_POINTER_BEARING_OBJECT_THRESHOLD_BYTES`), with margin.
const OLD_RESULT_PARTS: usize = (LARGE_POINTER_BEARING_OBJECT_THRESHOLD_BYTES / 8) * 2 + 17;

fn expected_part(i: usize) -> String {
    format!("w{i:x}")
}

/// `"w0 w1 w2 …"` with `parts` parts, as a heap string.
fn split_source(parts: usize) -> *mut crate::StringHeader {
    let mut text = String::new();
    for i in 0..parts {
        if i > 0 {
            text.push(' ');
        }
        text.push_str(&expected_part(i));
    }
    crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32)
}

fn split_on_space(source: *mut crate::StringHeader) -> *mut crate::array::ArrayHeader {
    let space = crate::string::js_string_from_bytes(b" ".as_ptr(), 1);
    crate::string::js_string_split(source, space)
}

unsafe fn part_ptr(arr: *const crate::array::ArrayHeader, i: usize) -> *const crate::StringHeader {
    let elements = crate::array::array_elements_ptr(arr) as *const u64;
    (*elements.add(i) & POINTER_MASK) as *const crate::StringHeader
}

/// Every part of the split rooted in shadow slot `slot` still reads back.
fn assert_parts_intact(slot: u32, parts: usize) {
    let arr = (js_shadow_slot_get(slot) & POINTER_MASK) as *const crate::array::ArrayHeader;
    unsafe {
        assert_eq!((*arr).length as usize, parts);
        for i in 0..parts {
            assert_string_bytes(part_ptr(arr, i), expected_part(i).as_bytes());
        }
    }
}

fn is_old(addr: usize) -> bool {
    matches!(
        crate::arena::classify_heap_generation(addr),
        crate::arena::HeapGeneration::Old
    )
}

#[test]
fn split_into_old_result_births_old_parts_and_remembers_nothing() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let source = split_source(OLD_RESULT_PARTS);
    js_shadow_slot_set(1, string_bits(source as usize));
    reset_remembered_set();
    let remembered_before = remembered_set_size();

    let arr = split_on_space(source);
    js_shadow_slot_set(0, ptr_bits(arr as usize));

    assert!(
        is_old(arr as usize),
        "premise: the result array is born old"
    );
    unsafe {
        for i in 0..OLD_RESULT_PARTS {
            let part = part_ptr(arr, i) as usize;
            assert!(
                is_old(part),
                "part {i} of an old-born result must be born old"
            );
            let flags = (*header_from_user_ptr(part as *const u8)).gc_flags;
            assert_ne!(
                flags & GC_FLAG_TENURED,
                0,
                "part {i} carries the born-old stamp"
            );
        }
    }
    assert_eq!(
        remembered_set_size(),
        remembered_before,
        "old→old stores into the fresh result must remember no slot"
    );

    let _ = collect_minor_trace(GcTriggerKind::Direct);
    assert_parts_intact(0, OLD_RESULT_PARTS);
    js_shadow_slot_set(1, 0);
    js_gc_collect();
    assert_parts_intact(0, OLD_RESULT_PARTS);
    js_shadow_slot_set(0, 0);
}

#[test]
fn split_into_young_result_keeps_young_parts() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let source = split_source(64);
    let arr = split_on_space(source);
    assert!(
        crate::arena::pointer_in_nursery(arr as usize),
        "premise: a 64-part result is born young"
    );
    unsafe {
        for i in 0..64 {
            let part = part_ptr(arr, i) as usize;
            assert!(
                crate::arena::pointer_in_nursery(part),
                "part {i} of a young result stays young"
            );
            assert_string_bytes(
                part as *const crate::StringHeader,
                expected_part(i).as_bytes(),
            );
        }
    }
}

/// The hard condition: parts born old WHILE a cycle is marking must be visible
/// to that mark (allocate-black), or its sweep frees them under a live result.
#[test]
fn split_into_old_result_during_active_mark_survives_the_cycle() {
    let _legacy_pacing = crate::gc::policy::force_legacy_gc_pacing();
    let _guard = CopyingNurseryTestGuard::new(2);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let old_in_use = crate::arena::old_gen_in_use_bytes();
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.set(old_in_use));
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));

    let source = split_source(OLD_RESULT_PARTS);
    js_shadow_slot_set(1, string_bits(source as usize));
    for _ in 0..(GC_MUTATOR_ASSIST_WORK_UNITS * 4) {
        let _ = young_leaf();
    }
    trigger_guard.make_arena_trigger_due();

    gc_check_trigger();
    let mut status = JsGcStepResult::default();
    assert_eq!(js_gc_step_status(&mut status), JS_GC_STEP_STATUS_ACTIVE);
    // Step into marking proper: the barrier is armed from here on.
    let mut reached_mark = false;
    for _ in 0..500_000 {
        let current = js_gc_step_status(&mut status);
        assert_eq!(
            current, JS_GC_STEP_STATUS_ACTIVE,
            "cycle ended before marking"
        );
        if status.phase == GcCyclePhase::MarkPropagation.ffi_code() {
            reached_mark = true;
            break;
        }
        js_gc_step_work_units(1, &mut status);
    }
    assert!(reached_mark, "budgeted cycle never reached MarkPropagation");

    let source = (js_shadow_slot_get(1) & POINTER_MASK) as *mut crate::StringHeader;
    let arr = split_on_space(source);
    js_shadow_slot_set(0, ptr_bits(arr as usize));

    assert_eq!(
        js_gc_step_status(&mut status),
        JS_GC_STEP_STATUS_ACTIVE,
        "premise: the split ran inside the marking cycle"
    );
    assert!(
        is_old(arr as usize),
        "premise: the result array is born old"
    );
    unsafe {
        for i in 0..OLD_RESULT_PARTS {
            let part = part_ptr(arr, i) as usize;
            assert!(is_old(part), "part {i} must be born old");
            let flags = (*header_from_user_ptr(part as *const u8)).gc_flags;
            assert_ne!(
                flags & GC_FLAG_MARKED,
                0,
                "part {i} born old during marking must be born marked"
            );
        }
    }

    let completed = complete_budgeted_gc_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
    assert_parts_intact(0, OLD_RESULT_PARTS);

    // And a further full cycle (no allocate-black now) still finds them.
    js_shadow_slot_set(1, 0);
    js_gc_collect();
    assert_parts_intact(0, OLD_RESULT_PARTS);
    js_shadow_slot_set(0, 0);
}
