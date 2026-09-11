//! #9261: the collector's slot enumeration must reach every array element that
//! holds a heap reference.

use super::super::*;
use super::support::*;

use crate::gc::verify::{
    verify_array_pointer_slots_enumerated, verify_array_pointer_slots_enumerated_for,
    ArraySlotEnumerationStats,
};

/// Build a mask-described array: one numeric element, then one pointer.
///
/// The numeric element first is load-bearing. An EMPTY array's first pointer
/// append publishes `GC_LAYOUT_ALL_POINTERS` ("its sole element is the pointer
/// we just classified"), and under that state every element is enumerated by
/// construction — the omission this file is about cannot be expressed. With a
/// numeric prefix the append takes the per-object mask instead, which is the
/// state that can under-report.
fn mask_described_array() -> *mut crate::array::ArrayHeader {
    let arr = crate::array::js_array_alloc(8);
    let arr = crate::array::js_array_push_f64(arr, 1.0);
    let child = young_leaf();
    let arr = crate::array::js_array_push_f64(arr, f64::from_bits(ptr_bits(child)));
    let header = unsafe { header_from_user_ptr(arr as *const u8) };
    assert_eq!(
        unsafe { (*header)._reserved } & crate::gc::GC_LAYOUT_STATE_MASK,
        crate::gc::GC_LAYOUT_SIDE_MASK,
        "fixture must be described by a per-object pointer mask, or the \
         sabotage below is not expressible and every verdict here is vacuous"
    );
    arr
}

unsafe fn stats_for(arr: *mut crate::array::ArrayHeader) -> ArraySlotEnumerationStats {
    let mut stats = ArraySlotEnumerationStats::default();
    verify_array_pointer_slots_enumerated_for(&mut stats, header_from_user_ptr(arr as *const u8));
    stats
}

/// Publish a pointer at the append position WITHOUT the layout note every
/// store path performs. This is the state #9261 found in the wild on an
/// object's spill buffer: `mask=0xc7fc live=0xfffc`, three live `STRING_TAG`
/// elements the mask omitted. The check must SEE it — a checker that cannot
/// fail is documentation.
#[test]
fn array_slot_enumeration_reports_a_pointer_element_the_layout_omits() {
    let _isolation = copying_nursery_isolation_lock();
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let arr = mask_described_array();

    // The fixture itself must be clean, and must have LOOKED at something.
    let clean = unsafe { stats_for(arr) };
    assert_eq!(
        clean.unenumerated_slots, 0,
        "a correctly-noted array must not be reported"
    );
    assert_eq!(clean.checked_arrays, 1);
    assert!(
        clean.checked_pointer_slots >= 1,
        "no pointer element was examined, so the clean verdict above is vacuous"
    );

    let planted = young_leaf();
    let index = unsafe { (*arr).length } as usize;
    unsafe {
        let elements =
            crate::array::array_elements_ptr(arr as *const crate::array::ArrayHeader) as *mut u64;
        std::ptr::write(elements.add(index), ptr_bits(planted));
        (*arr).length = index as u32 + 1;
    }

    let broken = unsafe { stats_for(arr) };
    assert_eq!(
        broken.unenumerated_slots, 1,
        "the planted element is a live heap edge the collector cannot reach"
    );
    let missing = broken
        .first
        .expect("the first offending element is recorded");
    assert_eq!(missing.index, index);
    assert_eq!(missing.child, planted);
    assert_eq!(missing.array, arr as usize);

    clear_marks();
    remembered_set_clear();
}

/// The whole-heap form is what the copied minor calls, so it must actually
/// walk arrays rather than return an empty verdict.
#[test]
fn array_slot_enumeration_walks_the_heap() {
    let _isolation = copying_nursery_isolation_lock();
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let arr = mask_described_array();
    // The walk skips unmarked nursery objects (they are this cycle's garbage,
    // and their elements are the previous tenant's bytes). Nothing has marked
    // anything here, so pin the fixture to make it a live subject.
    let header = unsafe { header_from_user_ptr(arr as *const u8) };
    unsafe {
        crate::gc::pin_object(header);
    }

    let stats = verify_array_pointer_slots_enumerated();
    assert!(
        stats.checked_arrays >= 1 && stats.checked_pointer_slots >= 1,
        "the heap walk examined no array element ({stats:?}), so a clean \
         verdict from it would mean nothing"
    );

    unsafe {
        crate::gc::unpin_object(header);
    }
    clear_marks();
    remembered_set_clear();
}

extern "C" fn species_destination(
    closure: *const crate::closure::ClosureHeader,
    _length: f64,
) -> f64 {
    crate::closure::js_closure_get_capture_f64(closure, 0)
}

extern "C" fn interrupt_species_copy(_closure: *const crate::closure::ClosureHeader) -> f64 {
    crate::exception::js_throw(9983.0)
}

// Even a private array prototype sets these process-wide fast-path latches.
// As in dyn_eval's ArrayPrototypeLatchGuard, restore both once this fixture's
// arrays are unreachable, including when an assertion unwinds.
struct ArrayPrototypeLatchGuard {
    _guard_tests: std::sync::MutexGuard<'static, ()>,
    recorded: bool,
    invalidated: u8,
}

impl ArrayPrototypeLatchGuard {
    fn new() -> Self {
        let _guard_tests = crate::typed_feedback::typed_feedback_test_lock();
        Self {
            _guard_tests,
            recorded: crate::object::prototype_chain::array_static_proto_recorded(),
            invalidated: crate::array::PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED
                .load(std::sync::atomic::Ordering::Relaxed),
        }
    }
}

impl Drop for ArrayPrototypeLatchGuard {
    fn drop(&mut self) {
        crate::object::prototype_chain::test_swap_array_static_proto_recorded(self.recorded);
        crate::array::test_swap_array_index_fast_path_invalidated(self.invalidated);
    }
}

/// Inspect the exact custom destination after an indexed getter interrupts the
/// public runtime entry point. No collector invocation or optional diagnostic
/// wiring is needed: the scanner's enumeration is the assertion.
fn interrupted_species_copy_describes_late_pointer(splice: bool) {
    let _isolation = copying_nursery_isolation_lock();
    let _latches = ArrayPrototypeLatchGuard::new();
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let destination = crate::array::js_array_alloc_with_length(12);
    let old = young_leaf();
    crate::array::js_array_set_f64(destination, 0, f64::from_bits(ptr_bits(old)));
    for index in 1..12 {
        crate::array::js_array_set_f64(destination, index, index as f64);
    }
    let source = crate::array::js_array_alloc_with_length(12);
    for index in 0..10 {
        crate::array::js_array_set_f64(source, index, index as f64);
    }
    let late = young_leaf();
    crate::array::js_array_set_f64(source, 10, f64::from_bits(ptr_bits(late)));
    let species = crate::closure::js_closure_alloc(species_destination as *const u8, 1);
    crate::closure::js_closure_set_capture_f64(
        species,
        0,
        f64::from_bits(ptr_bits(destination as usize)),
    );
    let species_value = f64::from_bits(ptr_bits(species as usize));
    let symbol = crate::symbol::well_known_symbol("species");
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            species_value,
            f64::from_bits(ptr_bits(symbol as usize)),
            species_value,
        );
    }
    let key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
    crate::array::js_array_set_string_key(source, key, species_value);
    let getter = crate::closure::js_closure_alloc(interrupt_species_copy as *const u8, 0);
    let descriptor = crate::object::js_object_alloc(0, 0);
    let get_key = crate::string::js_string_from_bytes(b"get".as_ptr(), 3);
    crate::object::js_object_set_field_by_name(
        descriptor,
        get_key,
        f64::from_bits(ptr_bits(getter as usize)),
    );
    let index_key = crate::string::js_string_from_bytes(b"11".as_ptr(), 2);
    let prototype = crate::array::js_array_alloc_with_length(12);
    crate::object::js_object_define_property(
        f64::from_bits(ptr_bits(prototype as usize)),
        f64::from_bits(string_bits(index_key as usize)),
        f64::from_bits(ptr_bits(descriptor as usize)),
    );
    crate::object::js_object_set_prototype_of(
        f64::from_bits(ptr_bits(source as usize)),
        f64::from_bits(ptr_bits(prototype as usize)),
    );
    // An ordinary data descriptor makes slice take its observable-read path
    // without mutating the process-global canonical Array prototype. Splice
    // reads the inherited accessor through the source's actual hole at 11.
    let data_descriptor = crate::object::js_object_alloc(0, 0);
    let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
    crate::object::js_object_set_field_by_name(data_descriptor, value_key, 0.0);
    crate::object::js_object_define_property(
        f64::from_bits(ptr_bits(source as usize)),
        0.0,
        f64::from_bits(ptr_bits(data_descriptor as usize)),
    );
    assert!(crate::array::array_iteration_is_exotic(source));
    assert!(crate::array::array_spec_has_index(source, 11));
    let header = unsafe { header_from_user_ptr(destination as *const u8) };
    assert_eq!(
        unsafe { (*header)._reserved } & GC_LAYOUT_STATE_MASK,
        GC_LAYOUT_SIDE_MASK,
        "the original destination must have a per-object pointer mask"
    );
    let before = unsafe { stats_for(destination) };
    assert_eq!(before.checked_arrays, 1);
    assert_eq!(before.checked_pointer_slots, 1);
    assert_eq!(before.unenumerated_slots, 0);

    let interrupted = crate::exception::catch_js_throw(|| {
        if splice {
            let mut out = std::ptr::null_mut();
            crate::array::js_array_splice(source, 0, 12, std::ptr::null(), 0, &mut out)
        } else {
            crate::array::js_array_slice(source, 0, 12)
        }
    });
    crate::object::descriptor_state::clear_object_descriptors(source as usize);
    crate::object::descriptor_state::clear_object_descriptors(prototype as usize);
    crate::object::js_object_set_prototype_of(
        f64::from_bits(ptr_bits(source as usize)),
        f64::from_bits(crate::value::TAG_NULL),
    );
    unsafe {
        crate::symbol::js_object_delete_symbol_property(
            species_value,
            f64::from_bits(ptr_bits(symbol as usize)),
        );
    }
    assert_eq!(
        interrupted.expect_err("the indexed getter must interrupt the copy"),
        9983.0
    );
    assert_eq!(
        crate::array::js_array_get_f64(destination, 10).to_bits(),
        ptr_bits(late),
        "the exact late child must reach index 10 before interruption"
    );
    let after = unsafe { stats_for(destination) };
    assert_eq!(after.checked_arrays, 1);
    assert_eq!(after.checked_pointer_slots, 1);
    assert_eq!(
        after.unenumerated_slots, 0,
        "partial species result lost its late reference: {after:?}"
    );
    clear_marks();
    remembered_set_clear();
}

#[test]
fn interrupted_slice_species_result_keeps_its_late_pointer_enumerated() {
    interrupted_species_copy_describes_late_pointer(false);
}

#[test]
fn interrupted_splice_species_result_keeps_its_late_pointer_enumerated() {
    interrupted_species_copy_describes_late_pointer(true);
}
