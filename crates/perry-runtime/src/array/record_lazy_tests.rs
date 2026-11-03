//! `for..of` over a lazy `JSON.parse` array: the record admits it without
//! forcing it, steps its elements through the lazy read policy, and follows
//! the materialized head once that policy installs one. The protocol arm
//! (`js_get_iterator` + one IteratorStep per element) is never taken.
use super::iterator_stack_record::{js_array_record_stack_dispatch, ArrayStackRecord};
use super::iterator_step::js_array_record_stack_enter;
use crate::value::TAG_UNDEFINED;

const FACTS: u32 = super::ITERATION_MEMBER_WRITABLE | super::ITERATION_NEXT_REACHABLE;

fn lazy_array_box(n: u32) -> f64 {
    let items: Vec<String> = (0..n).map(|i| format!("{{\"v\":{i}}}")).collect();
    let bytes = format!("[{}]", items.join(",")).into_bytes();
    let tape = crate::json_tape::build_tape(&bytes).unwrap();
    let text = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    let lazy = unsafe { crate::json_tape::alloc_lazy_array(&tape.entries, 0, n, text) };
    assert_eq!(
        obj_type(crate::value::js_nanbox_pointer(lazy as i64)),
        crate::gc::GC_TYPE_LAZY_ARRAY,
        "fixture must be a real lazy array or the assertions below are vacuous"
    );
    crate::value::js_nanbox_pointer(lazy as i64)
}

fn obj_type(value: f64) -> u8 {
    let raw = (value.to_bits() & crate::value::POINTER_MASK) as usize;
    unsafe { crate::value::addr_class::try_read_gc_header(raw) }
        .expect("a GC-headed record payload")
        .obj_type
}

fn record(payload: f64) -> ArrayStackRecord {
    ArrayStackRecord {
        payload,
        next: f64::from_bits(TAG_UNDEFINED),
        index: 0.0,
        state: 0.0,
        protocol: 0.0,
        value: f64::from_bits(TAG_UNDEFINED),
    }
}

#[test]
fn record_admits_a_lazy_array_and_follows_its_materialized_head() {
    // Admission is refused while the process-wide "an array's prototype was
    // retargeted" latch is set; another test in the process may have set it.
    let _latch = crate::test_support::ArrayPrototypeLatchGuard::new();
    crate::object::prototype_chain::test_swap_array_static_proto_recorded(false);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let scope = crate::gc::RuntimeHandleScope::new();
    let n = 300u32;
    let lazy = scope.root_nanbox_f64(lazy_array_box(n));
    let lazy_bits = lazy.get_nanbox_f64().to_bits();
    let raw = (lazy_bits & crate::value::POINTER_MASK) as usize;

    let mut rec = record(lazy.get_nanbox_f64());
    let verdict = unsafe { js_array_record_stack_enter(&mut rec, FACTS, 1) };
    assert_eq!(verdict, 0, "a lazy array is admitted to the indexed head");
    assert_eq!(
        rec.payload.to_bits(),
        lazy_bits,
        "entry does not replace it"
    );
    assert!(
        unsafe { crate::json_tape::is_lazy_array(raw as *const crate::json_tape::LazyArrayHeader) },
        "entry must not materialize: an early break keeps the laziness"
    );

    // Step every element through the cold indexed step, as the compiled head
    // does while its inline guard rejects the lazy header.
    let threshold = crate::json_tape::scan_flip_threshold(n);
    let mut seen = Vec::new();
    for i in 0..n {
        rec.index = i as f64;
        let done = unsafe { js_array_record_stack_dispatch(&mut rec, 6) };
        assert_eq!(
            done.to_bits(),
            crate::value::TAG_FALSE,
            "element {i} exists"
        );
        seen.push(rec.value.to_bits());
        if i + 1 < threshold {
            assert_eq!(
                obj_type(rec.payload),
                crate::gc::GC_TYPE_LAZY_ARRAY,
                "before the policy flips, element {i} is read lazily"
            );
        }
    }
    assert_eq!(
        obj_type(rec.payload),
        crate::gc::GC_TYPE_ARRAY,
        "after the scan flip the record continues from the materialized head"
    );
    // Exhaustion closes the record (its payload is no longer the array), so
    // keep the head the loop ran on first.
    let head = rec.payload;
    rec.index = n as f64;
    let done = unsafe { js_array_record_stack_dispatch(&mut rec, 6) };
    assert_eq!(
        done.to_bits(),
        crate::value::TAG_TRUE,
        "exhausted at length"
    );
    for (i, bits) in seen.iter().enumerate() {
        let again = super::js_array_get_f64(
            crate::value::js_nanbox_get_pointer(head) as *const super::ArrayHeader,
            i as u32,
        );
        assert_eq!(again.to_bits(), *bits, "element {i} keeps its identity");
    }

    // A second loop over the same (now materialized) lazy value enters on the
    // materialized head directly: the inline indexed head from element 0.
    let mut rec = record(lazy.get_nanbox_f64());
    let verdict = unsafe { js_array_record_stack_enter(&mut rec, FACTS, 1) };
    assert_eq!(verdict, 0);
    assert_eq!(obj_type(rec.payload), crate::gc::GC_TYPE_ARRAY);
    assert_eq!(rec.payload.to_bits(), head.to_bits());
}

#[test]
fn record_read_step_follows_the_materialized_head_of_a_lazy_payload() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let scope = crate::gc::RuntimeHandleScope::new();
    let lazy = scope.root_nanbox_f64(lazy_array_box(200));
    let mut rec = record(lazy.get_nanbox_f64());
    assert_eq!(unsafe { js_array_record_stack_enter(&mut rec, 0, 0) }, 0);
    rec.index = 7.0;
    let first = unsafe { js_array_record_stack_dispatch(&mut rec, 5) };
    assert_eq!(
        obj_type(rec.payload),
        crate::gc::GC_TYPE_LAZY_ARRAY,
        "one read stays lazy"
    );
    // Any full-array operation materializes; the next cold read moves on.
    let raw = (lazy.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK) as usize;
    let _ = unsafe {
        crate::json_hooks::force_materialize_lazy(raw as *mut crate::json_tape::LazyArrayHeader)
    };
    let again = unsafe { js_array_record_stack_dispatch(&mut rec, 5) };
    assert_eq!(again.to_bits(), first.to_bits(), "same element object");
    assert_eq!(obj_type(rec.payload), crate::gc::GC_TYPE_ARRAY);
}
