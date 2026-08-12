//! #7949 — JS values retained in ordinary Rust containers across allocating
//! calls.
//!
//! ## What these tests have to prove
//!
//! Not "the program didn't crash". A rooting fix is only proven by a value that
//! **survived a collection that actually moved it**: every assertion here runs
//! under a `CopyingNurseryTestGuard`, forces a copying minor with
//! `collect_minor_trace`, and asserts `copied_objects > 0` before believing the
//! survival assertions. A cycle that moved nothing would satisfy the survival
//! assertions vacuously — that is the shape CLAUDE.md calls a presence check
//! rather than a proof.
//!
//! `plain_vec_of_values_is_not_a_root` is the sabotage arm, and it is what makes
//! the rest non-vacuous: the *same* objects, held in a bare `Vec<f64>` instead
//! of a `RootedValues`, keep naming their pre-collection addresses. If the
//! instrument could not tell the two apart, that test would fail.

use super::super::*;
use super::support::*;

use crate::gc::{RootedValues, RuntimeHandleScope};

/// Allocate a young string whose bytes identify it, so a survival assertion can
/// check the value is still the *same object* rather than merely a plausible
/// address.
fn young_named_string(name: &str) -> usize {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32) as usize
}

fn string_value(name: &str) -> f64 {
    f64::from_bits(string_bits(young_named_string(name)))
}

unsafe fn string_ptr_of(value: f64) -> *const crate::StringHeader {
    (value.to_bits() & POINTER_MASK) as *const crate::StringHeader
}

fn register_handle_scanner() {
    gc_register_mutable_root_scanner_with_source(
        scan_runtime_handle_roots_mut,
        MutableRootScannerSource::RuntimeHandles,
    );
}

#[test]
fn rooted_values_elements_survive_a_collection_that_moved_them() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_handle_scanner();

    let scope = RuntimeHandleScope::new();
    let mut values = RootedValues::new(&scope);
    let mut before = Vec::new();
    for i in 0..8 {
        let name = format!("rooted_values_{i}");
        let value = string_value(&name);
        before.push((value.to_bits() & POINTER_MASK) as usize);
        values.push(value);
    }

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert!(
        trace.copying_nursery.copied_objects > 0,
        "the cycle moved nothing -- the survival assertions below would be vacuous"
    );
    for (i, old) in before.iter().enumerate() {
        let value = values.get(i);
        let new = (value.to_bits() & POINTER_MASK) as usize;
        assert_ne!(
            new, *old,
            "element {i} was not relocated -- this cycle proves nothing about rooting"
        );
        unsafe {
            assert_string_bytes(
                string_ptr_of(value),
                format!("rooted_values_{i}").as_bytes(),
            );
        }
    }
}

#[test]
fn plain_vec_of_values_is_not_a_root() {
    // The sabotage arm for the test above: the *identical* workload with a bare
    // `Vec<f64>` accumulator. The collector cannot see the Rust heap, so the
    // words are left naming from-space. This is #7949 in one assertion, and it
    // is what proves the rooted test is measuring rooting rather than an
    // allocator that happened not to move anything.
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_handle_scanner();

    // One rooted witness so the cycle has something to relocate and we can
    // prove relocation happened at all.
    let scope = RuntimeHandleScope::new();
    let witness = scope.root_nanbox_f64(string_value("witness"));
    let witness_before = (witness.get_nanbox_f64().to_bits() & POINTER_MASK) as usize;

    let mut unrooted: Vec<f64> = Vec::new();
    for i in 0..8 {
        unrooted.push(string_value(&format!("unrooted_{i}")));
    }
    let before: Vec<usize> = unrooted
        .iter()
        .map(|v| (v.to_bits() & POINTER_MASK) as usize)
        .collect();

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert!(trace.copying_nursery.copied_objects > 0);
    assert_ne!(
        (witness.get_nanbox_f64().to_bits() & POINTER_MASK) as usize,
        witness_before,
        "a rooted value did not move -- this cycle cannot demonstrate the unrooted hazard"
    );
    for (i, value) in unrooted.iter().enumerate() {
        assert_eq!(
            (value.to_bits() & POINTER_MASK) as usize,
            before[i],
            "a plain Vec<f64> element was rewritten -- if this ever passes, the \
             collector grew a way to see the Rust heap and RootedValues can be retired"
        );
    }
}

// ---------------------------------------------------------------------------
// End-to-end: `Object.groupBy` / `Map.groupBy` with a collection inside the
// user callback -- the exact window #7949 names.
// ---------------------------------------------------------------------------

thread_local! {
    static GROUP_BY_COPIED_OBJECTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Stands in for a user callback that allocates enough to trigger a GC: it
/// forces a copying minor on every element, which is where a `Vec<f64>` of
/// already-collected `(key, item)` pairs goes stale.
extern "C" fn group_by_moving_callback(
    _closure: *const crate::closure::ClosureHeader,
    _item: f64,
    index: f64,
) -> f64 {
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    GROUP_BY_COPIED_OBJECTS.with(|c| c.set(c.get() + trace.copying_nursery.copied_objects));
    // A freshly allocated key string per call, so the key path allocates too.
    if (index as u64) % 2 == 0 {
        string_value("even")
    } else {
        string_value("odd")
    }
}

const GROUP_BY_ELEMENTS: usize = 6;

/// Build `["gb_0", ..., "gb_5"]` rooted in `scope`, plus the callback closure.
fn group_by_inputs(scope: &RuntimeHandleScope) -> (f64, f64) {
    let items = crate::array::js_array_alloc(GROUP_BY_ELEMENTS as u32);
    let items_handle = scope.root_nanbox_f64(f64::from_bits(ptr_bits(items as usize)));
    for i in 0..GROUP_BY_ELEMENTS {
        // Allocate the element FIRST, then re-read the array out of its root:
        // `js_string_from_bytes` can move the array, and `js_array_push` can
        // move it again by growing it.
        let element = crate::value::JSValue::from_bits(string_value(&format!("gb_{i}")).to_bits());
        let arr = (items_handle.get_nanbox_f64().to_bits() & POINTER_MASK)
            as *mut crate::array::ArrayHeader;
        let grown = crate::array::js_array_push(arr, element);
        items_handle.set_nanbox_f64(f64::from_bits(ptr_bits(grown as usize)));
    }
    let closure = crate::closure::js_closure_alloc(group_by_moving_callback as *const u8, 0);
    let callback_handle = scope.root_nanbox_f64(f64::from_bits(ptr_bits(closure as usize)));
    (
        items_handle.get_nanbox_f64(),
        callback_handle.get_nanbox_f64(),
    )
}

unsafe fn assert_group_contents(group_value: f64, expected: &[usize]) {
    let arr = (group_value.to_bits() & POINTER_MASK) as *const crate::array::ArrayHeader;
    assert!(!arr.is_null(), "group is not an array");
    assert_eq!(
        crate::array::js_array_length(arr) as usize,
        expected.len(),
        "group length"
    );
    for (slot, original_index) in expected.iter().enumerate() {
        let element = crate::array::js_array_get_f64(arr, slot as u32);
        assert_string_bytes(
            string_ptr_of(element),
            format!("gb_{original_index}").as_bytes(),
        );
    }
}

#[test]
fn object_group_by_items_survive_a_moving_minor_in_the_callback() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_handle_scanner();
    GROUP_BY_COPIED_OBJECTS.with(|c| c.set(0));

    let scope = RuntimeHandleScope::new();
    let (items_value, callback) = group_by_inputs(&scope);

    let result = crate::object::js_object_group_by(items_value, callback);
    let result_handle = scope.root_nanbox_f64(result);

    assert!(
        GROUP_BY_COPIED_OBJECTS.with(|c| c.get()) > 0,
        "no object was relocated during the callback -- the run proves nothing"
    );
    unsafe {
        let even = crate::value::js_get_property(
            result_handle.get_nanbox_f64(),
            b"even".as_ptr() as i64,
            4,
        );
        let odd = crate::value::js_get_property(
            result_handle.get_nanbox_f64(),
            b"odd".as_ptr() as i64,
            3,
        );
        assert_group_contents(even, &[0, 2, 4]);
        assert_group_contents(odd, &[1, 3, 5]);
    }
}

#[test]
fn map_group_by_items_survive_a_moving_minor_in_the_callback() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_handle_scanner();
    GROUP_BY_COPIED_OBJECTS.with(|c| c.set(0));

    let scope = RuntimeHandleScope::new();
    let (items_value, callback) = group_by_inputs(&scope);

    let result = crate::object::js_map_group_by(items_value, callback);
    let result_handle = scope.root_nanbox_f64(result);

    assert!(
        GROUP_BY_COPIED_OBJECTS.with(|c| c.get()) > 0,
        "no object was relocated during the callback -- the run proves nothing"
    );
    unsafe {
        let map =
            (result_handle.get_nanbox_f64().to_bits() & POINTER_MASK) as *mut crate::map::MapHeader;
        assert_eq!(crate::map::js_map_size(map), 2);
        let even = crate::map::js_map_get(map, string_value("even"));
        let map =
            (result_handle.get_nanbox_f64().to_bits() & POINTER_MASK) as *mut crate::map::MapHeader;
        let odd = crate::map::js_map_get(map, string_value("odd"));
        assert_group_contents(even, &[0, 2, 4]);
        assert_group_contents(odd, &[1, 3, 5]);
    }
}
