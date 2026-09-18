//! Argument lists a runtime dispatcher holds across an allocation.
//!
//! `Reflect.apply` and the rest/`arguments` bundler both read a call's
//! arguments into Rust locals and then allocate — a rebound closure, an index
//! key, the `arguments` array — before the callee ever sees them. A local is
//! not a GC root, so a moving collection in that window leaves the callee with
//! from-space addresses while the values themselves live on somewhere else.
//!
//! An allocation-trigger test cannot aim at one allocation inside one call, so
//! each test arms the named collection point that stands for it
//! (`gc::collection_points`) and then asserts the callee received each value's
//! POST-collection location. Every test also asserts its premise — a copying
//! minor ran and the value really moved — because "the callee saw the right
//! address" passes vacuously if nothing moved.

use super::*;
use crate::ObjectHeader;
use std::cell::RefCell;

crate::perry_thread_local! {
    static SEEN: RefCell<Vec<u64>> = RefCell::new(Vec::new());
}

fn record(values: &[u64]) {
    SEEN.with(|seen| seen.borrow_mut().extend_from_slice(values));
}

fn seen() -> Vec<u64> {
    SEEN.with(|seen| std::mem::take(&mut *seen.borrow_mut()))
}

extern "C" fn record_this_and_six_args(
    _closure: *const crate::closure::ClosureHeader,
    a0: f64,
    a1: f64,
    _a2: f64,
    _a3: f64,
    _a4: f64,
    a5: f64,
) -> f64 {
    record(&[
        crate::object::js_implicit_this_get().to_bits(),
        a0.to_bits(),
        a1.to_bits(),
        a5.to_bits(),
    ]);
    0.0
}

extern "C" fn record_two_args(
    _closure: *const crate::closure::ClosureHeader,
    a0: f64,
    a1: f64,
) -> f64 {
    record(&[a0.to_bits(), a1.to_bits()]);
    0.0
}

#[allow(clippy::too_many_arguments)]
extern "C" fn record_rest_and_arguments_after_16(
    _closure: *const crate::closure::ClosureHeader,
    _a0: f64,
    _a1: f64,
    _a2: f64,
    _a3: f64,
    _a4: f64,
    _a5: f64,
    _a6: f64,
    _a7: f64,
    _a8: f64,
    _a9: f64,
    _a10: f64,
    _a11: f64,
    _a12: f64,
    _a13: f64,
    _a14: f64,
    _a15: f64,
    rest: f64,
    arguments: f64,
) -> f64 {
    let rest_ptr = (rest.to_bits() & POINTER_MASK) as *const crate::array::ArrayHeader;
    let arguments_ptr = (arguments.to_bits() & POINTER_MASK) as *const crate::array::ArrayHeader;
    let rest_len = crate::array::js_array_length(rest_ptr);
    record(&[u64::from(rest_len)]);
    // A from-space read under `PoisonOnly` returns the poison word, so the
    // length above is the discriminating observation; only read elements when
    // it is the length this call really has.
    if rest_len == 2 {
        record(&[
            crate::array::js_array_get(rest_ptr, 0).bits(),
            u64::from(crate::array::js_array_length(arguments_ptr)),
            crate::array::js_array_get(arguments_ptr, 16).bits(),
        ]);
    }
    0.0
}

/// A closure the armed minor must NOT move: the callee identity is not what
/// these tests are about, and a from-space callee pointer would throw
/// "value is not a function" out of a unit test instead of failing an
/// assertion. Two minors under a pinned promotion age tenure it into the
/// non-moving old generation.
fn tenured_closure(body: *const u8, capture_count: u32) -> *mut crate::closure::ClosureHeader {
    let closure = crate::closure::js_closure_alloc(body, capture_count);
    let scope = RuntimeHandleScope::new();
    let handle = scope.root_raw_mut_ptr(closure);
    {
        let _tenuring = crate::gc::tenuring::set_survivals_for_test(1);
        let _ = gc_collect_minor();
        let _ = gc_collect_minor();
    }
    // #7341: nothing allocates after this read within the function (the two
    // minors above already ran), so it is the "final read in a scope with
    // nothing after it" the ratchet's own docstring carves out — but
    // `with_mut_ptr` keeps it out of the raw-handle debt count without
    // changing behavior: the closure runs immediately and hands the pointer
    // straight back.
    let tenured = handle.with_mut_ptr::<crate::closure::ClosureHeader, _>(|ptr| ptr);
    assert!(
        !crate::arena::pointer_in_nursery(tenured as usize),
        "premise: the callee must be out of the nursery so only the arguments move"
    );
    tenured
}

fn array_value(values: &[f64]) -> f64 {
    let mut arr = crate::array::js_array_alloc(values.len() as u32);
    for value in values {
        arr = crate::array::js_array_push_f64(arr, *value);
    }
    f64::from_bits(ptr_bits(arr as usize))
}

fn array_element(value: f64, index: u32) -> u64 {
    let arr = (value.to_bits() & POINTER_MASK) as *const crate::array::ArrayHeader;
    crate::array::js_array_get(arr, index).bits()
}

/// #10532 review finding: `Reflect.apply` held the callee, the receiver and
/// every argument in plain Rust locals while `rebind_explicit_this` allocated a
/// rebound closure. A collection there left the callee reading from-space
/// addresses for all three.
#[test]
fn reflect_apply_roots_receiver_and_arguments_across_the_rebind_allocation() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();

    // A concise/object-literal method — a non-arrow closure with a reserved
    // `this` capture — is the shape whose rebind CLONES, and that clone is the
    // allocation this test collects inside.
    let closure = tenured_closure(
        record_this_and_six_args as *const u8,
        crate::closure::CAPTURES_THIS_FLAG | 1,
    );
    let scope = RuntimeHandleScope::new();
    let callee = scope.root_nanbox_f64(f64::from_bits(ptr_bits(closure as usize)));
    let receiver = scope.root_nanbox_f64(f64::from_bits(ptr_bits(crate::object::js_object_alloc(
        0, 0,
    ) as usize)));
    let receiver_original = (receiver.get_nanbox_f64().to_bits() & POINTER_MASK) as usize;
    let list = scope.root_nanbox_f64(array_value(&[
        test_string_value(b"first"),
        test_string_value(b"second"),
        2.0,
        3.0,
        4.0,
        test_string_value(b"sixth"),
    ]));
    let first_original = (array_element(list.get_nanbox_f64(), 0) & POINTER_MASK) as usize;

    crate::gc::arm_collection_point("reflect.apply.rebind");
    let before = crate::gc::copying_minor_cycles();
    crate::proxy::js_reflect_apply(
        callee.get_nanbox_f64(),
        receiver.get_nanbox_f64(),
        list.get_nanbox_f64(),
    );

    assert!(
        crate::gc::copying_minor_cycles() > before,
        "premise: the armed collection point ran a copying minor"
    );
    let receiver_now = receiver.get_nanbox_f64();
    let list_now = list.get_nanbox_f64();
    assert_ne!(
        (receiver_now.to_bits() & POINTER_MASK) as usize,
        receiver_original,
        "premise: the receiver moved"
    );
    assert_ne!(
        (array_element(list_now, 0) & POINTER_MASK) as usize,
        first_original,
        "premise: the first argument moved"
    );
    assert_eq!(
        seen(),
        vec![
            receiver_now.to_bits(),
            array_element(list_now, 0),
            array_element(list_now, 1),
            array_element(list_now, 5),
        ],
        "Reflect.apply must hand the callee the post-collection receiver and \
         arguments, not the addresses they had before the rebind allocated"
    );
}

/// #10532 review finding: `CreateListFromArrayLike`'s array-like path allocated
/// an index key per element while the source object and the elements collected
/// so far sat in Rust locals.
#[test]
fn array_like_argument_lists_root_the_source_and_the_collected_elements() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();

    let closure = tenured_closure(record_two_args as *const u8, 0);
    let scope = RuntimeHandleScope::new();
    let callee = scope.root_nanbox_f64(f64::from_bits(ptr_bits(closure as usize)));
    let source = crate::object::js_object_alloc(0, 3);
    let source_value = scope.root_nanbox_f64(f64::from_bits(ptr_bits(source as usize)));
    for (name, value) in [
        (&b"length"[..], 2.0),
        (&b"0"[..], test_string_value(b"zero")),
        (&b"1"[..], test_string_value(b"one")),
    ] {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let obj = (source_value.get_nanbox_f64().to_bits() & POINTER_MASK) as *mut ObjectHeader;
        crate::object::js_object_set_field_by_name(obj, key, value);
    }
    let zero_original = (element_by_name(&source_value, b"0") & POINTER_MASK) as usize;

    crate::gc::arm_collection_point("reflect.list_from_array_like.index_key");
    let before = crate::gc::copying_minor_cycles();
    crate::proxy::js_reflect_apply(
        callee.get_nanbox_f64(),
        f64::from_bits(crate::value::TAG_UNDEFINED),
        source_value.get_nanbox_f64(),
    );

    assert!(
        crate::gc::copying_minor_cycles() > before,
        "premise: the armed collection point ran a copying minor"
    );
    assert_ne!(
        (element_by_name(&source_value, b"0") & POINTER_MASK) as usize,
        zero_original,
        "premise: the element moved"
    );
    assert_eq!(
        seen(),
        vec![
            element_by_name(&source_value, b"0"),
            element_by_name(&source_value, b"1"),
        ],
        "an array-like argument list must be read out of the post-collection \
         source object"
    );
}

fn element_by_name(source: &RuntimeHandle<'_>, name: &[u8]) -> u64 {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let obj = (source.get_nanbox_f64().to_bits() & POINTER_MASK) as *const ObjectHeader;
    crate::object::js_object_get_field_by_name_f64(obj, key).to_bits()
}

/// #10532 review finding: a `(…fixed, ...rest)` body that also takes a
/// synthesized `arguments` builds TWO arrays. The second allocation could move
/// the first, and the bundler passed the callee the address the first array had
/// before it moved. From-space is poisoned here so a stale read is visible as a
/// wrong `rest.length` instead of intact bytes that happen to still be there.
#[test]
fn rest_bundling_roots_the_rest_array_across_the_arguments_array() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    let _protection =
        crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    register_runtime_handle_root_scanner_for_tests();

    let body = record_rest_and_arguments_after_16 as *const u8;
    crate::closure::js_register_closure_rest_and_arguments(body, 16);
    let closure = crate::closure::js_closure_alloc(body, 0);
    let scope = RuntimeHandleScope::new();
    let closure_handle = scope.root_raw_mut_ptr(closure);
    let tail = scope.root_nanbox_f64(test_string_value(b"rest-tail"));
    let tail_original = (tail.get_nanbox_f64().to_bits() & POINTER_MASK) as usize;

    let mut args: Vec<f64> = (0..16).map(f64::from).collect();
    args.push(tail.get_nanbox_f64());
    args.push(99.0);

    crate::gc::arm_collection_point("closure.rest_bundle.between_arrays");
    let before = crate::gc::copying_minor_cycles();
    let retired_before = crate::arena::quarantine_stats().sets_retired;
    // #7341: `js_closure_call_array` is itself the self-rooting entry point
    // under test here, so the closure pointer is a scoped argument to it —
    // `with_mut_ptr` is the blessed shape for that instead of a bare read.
    closure_handle.with_mut_ptr::<crate::closure::ClosureHeader, _>(|ptr| unsafe {
        crate::closure::js_closure_call_array(ptr as i64, args.as_ptr(), args.len() as i64);
    });

    assert!(
        crate::gc::copying_minor_cycles() > before,
        "premise: the armed collection point ran a copying minor"
    );
    assert!(
        crate::arena::quarantine_stats().sets_retired > retired_before,
        "premise: the minor retired from-space, so a stale read finds poison"
    );
    assert_ne!(
        (tail.get_nanbox_f64().to_bits() & POINTER_MASK) as usize,
        tail_original,
        "premise: the trailing argument moved"
    );
    assert_eq!(
        seen(),
        vec![
            2,
            tail.get_nanbox_f64().to_bits(),
            18,
            tail.get_nanbox_f64().to_bits()
        ],
        "the rest array and the arguments object must both be the post-collection \
         arrays, holding the post-collection argument values"
    );
}

/// #10532 review (round 2): a raw, untagged heap-pointer bit pattern (the
/// Promise executor's resolve/reject shape, `top16 == 0`) stored as an
/// array-like element is exactly as movable as a NaN-boxed pointer, but
/// `JSValue::is_pointer()` does not recognize it, and `root_nanbox_f64`'s
/// `Nanbox` scanner only rewrites POINTER_TAG/STRING_TAG/BIGINT_TAG bit
/// patterns -- it would silently do nothing for a raw one.
#[test]
fn array_like_argument_lists_root_raw_untagged_heap_pointer_elements() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();

    let callee = tenured_closure(record_two_args as *const u8, 0);
    let scope = RuntimeHandleScope::new();
    let callee_handle = scope.root_nanbox_f64(f64::from_bits(ptr_bits(callee as usize)));

    // A closure left in the nursery, referenced ONLY by its raw (unboxed)
    // address -- the exact shape `js_promise_new_with_executor` hands a
    // user's executor for `resolve`/`reject` (see proxy.rs's
    // `ValueMoveKind::RawHeapWord` doc comment).
    let raw_closure = crate::closure::js_closure_alloc(record_two_args as *const u8, 0);
    let observer = scope.root_raw_mut_ptr(raw_closure);
    let raw_bits_before = raw_closure as usize as u64;

    // Exactly 2 elements to match `record_two_args`'s declared arity -- an
    // under-applied raw extern "C" test body has no registered arity to pad
    // against, so this keeps the call itself unremarkable and isolates the
    // one thing under test: whether element 0 survives as a raw heap word.
    let source = crate::object::js_object_alloc(0, 3);
    let source_value = scope.root_nanbox_f64(f64::from_bits(ptr_bits(source as usize)));
    for (name, value) in [
        (&b"length"[..], 2.0),
        (&b"0"[..], f64::from_bits(raw_bits_before)),
        (&b"1"[..], 7.0),
    ] {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let obj = (source_value.get_nanbox_f64().to_bits() & POINTER_MASK) as *mut ObjectHeader;
        crate::object::js_object_set_field_by_name(obj, key, value);
    }

    // Fire the forced collection on the SECOND loop iteration (reading
    // element "1"), not the first: element "0"'s raw pointer is read on the
    // first iteration and, without this fix, copied bare into `out[0]` with
    // nothing rooting it. Firing the collection a step later is what puts
    // that already-read copy at risk, instead of the collection landing
    // before element "0" is ever read (which every read would trivially
    // survive, fix or no fix).
    crate::gc::arm_collection_point_after("reflect.list_from_array_like.index_key", 1);
    let before = crate::gc::copying_minor_cycles();
    crate::proxy::js_reflect_apply(
        callee_handle.get_nanbox_f64(),
        f64::from_bits(crate::value::TAG_UNDEFINED),
        source_value.get_nanbox_f64(),
    );

    assert!(
        crate::gc::copying_minor_cycles() > before,
        "premise: the armed collection point ran a copying minor"
    );
    // #7341: nothing allocates after this read; `with_mut_ptr` keeps it out
    // of the raw-handle debt count (see `tenured_closure` above).
    let raw_bits_after =
        observer.with_mut_ptr::<crate::closure::ClosureHeader, _>(|ptr| ptr as usize as u64);
    assert_ne!(
        raw_bits_after, raw_bits_before,
        "premise: the raw-bit closure moved"
    );
    assert_eq!(
        seen(),
        vec![raw_bits_after, 7.0_f64.to_bits()],
        "element \"0\" must be the post-collection raw address, not the \
         pre-collection one read before the later collection at element \"1\""
    );
}
