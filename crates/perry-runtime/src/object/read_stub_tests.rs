//! #10768: the read stub's probe and prime run ONE receiver guard.
//!
//! Every refusal below comes with a positive control on the same receiver and
//! key. Without it, a guard that refused every receiver would pass all of
//! these tests.

use super::*;
use crate::object::{js_object_alloc, js_object_set_field};
use crate::value::JSValue;

/// An arbitrary nonzero class id: the stub declines class-less receivers.
const TEST_CLASS_ID: u32 = 10_768;

/// A receiver the stub serves: a real class id, a stamped shape, and inline
/// slot 0 holding `value`.
unsafe fn served_receiver(value: f64) -> *mut ObjectHeader {
    let obj = js_object_alloc(TEST_CLASS_ID, 2);
    js_object_set_field(obj, 0, JSValue::number(value));
    assert!(
        stub_receiver_token(obj).is_some(),
        "fixture premise: the stub serves this receiver, or every refusal below is vacuous"
    );
    obj
}

/// Content bits for a short key. Each test uses its own names, so an entry
/// another test left in this thread's table cannot answer here.
fn key_bits(name: &[u8]) -> u64 {
    JSValue::try_short_string(name)
        .expect("test keys fit the inline form")
        .bits()
}

unsafe fn gc_header(obj: *mut ObjectHeader) -> *mut crate::gc::GcHeader {
    crate::value::addr_class::try_read_tracked_gc_header(obj as usize)
        .expect("a freshly allocated object carries a tracked GcHeader")
        .as_ptr()
}

/// A condition under which the stub must not serve a receiver. `apply` and
/// `revert` toggle it in place, so the shape token is the same either way.
struct Refusal {
    name: &'static str,
    apply: unsafe fn(*mut ObjectHeader),
    revert: unsafe fn(*mut ObjectHeader),
}

const REFUSALS: [Refusal; 5] = [
    Refusal {
        name: "GC_FLAG_FORWARDED",
        apply: |o| unsafe { (*gc_header(o)).gc_flags |= crate::gc::GC_FLAG_FORWARDED },
        revert: |o| unsafe { (*gc_header(o)).gc_flags &= !crate::gc::GC_FLAG_FORWARDED },
    },
    Refusal {
        name: "OBJ_FLAG_HAS_DESCRIPTORS",
        apply: |o| unsafe { (*gc_header(o))._reserved |= crate::gc::OBJ_FLAG_HAS_DESCRIPTORS },
        revert: |o| unsafe { (*gc_header(o))._reserved &= !crate::gc::OBJ_FLAG_HAS_DESCRIPTORS },
    },
    Refusal {
        name: "OBJ_FLAG_TYPED_ARRAY_PROTO",
        apply: |o| unsafe { (*gc_header(o))._reserved |= crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO },
        revert: |o| unsafe { (*gc_header(o))._reserved &= !crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO },
    },
    Refusal {
        name: "class_id == 0",
        apply: |o| unsafe { (*o).class_id = 0 },
        revert: |o| unsafe { (*o).class_id = TEST_CLASS_ID },
    },
    Refusal {
        name: "NATIVE_MODULE_CLASS_ID",
        apply: |o| unsafe { (*o).class_id = crate::object::NATIVE_MODULE_CLASS_ID },
        revert: |o| unsafe { (*o).class_id = TEST_CLASS_ID },
    },
];

/// The issue's first gap: the prime used to install an entry for a receiver
/// carrying `GC_FLAG_FORWARDED`, which the probe would never serve.
#[test]
fn prime_refuses_a_forwarded_receiver() {
    unsafe {
        let obj = served_receiver(41.0);
        let token = stub_receiver_token(obj).unwrap();
        let kb = key_bits(b"rsfw");
        assert_eq!(read_stub_probe(token, kb), None, "premise: a fresh key");

        (*gc_header(obj)).gc_flags |= crate::gc::GC_FLAG_FORWARDED;
        read_stub_prime(obj, kb, 0);
        (*gc_header(obj)).gc_flags &= !crate::gc::GC_FLAG_FORWARDED;
        assert_eq!(
            read_stub_probe(token, kb),
            None,
            "the prime filed an entry for a forwarded receiver"
        );

        // Positive control: the same prime on the same receiver, unforwarded.
        read_stub_prime(obj, kb, 0);
        assert_eq!(read_stub_probe(token, kb), Some(0));
        assert_eq!(read_stub_lookup(obj, kb), Some(41.0));
    }
}

/// Probe and prime agree on EVERY refusal, not just the forwarded one: under
/// each condition the prime files nothing, and an entry that already exists
/// is not served. Reverting the condition serves that entry again, which is
/// the positive control for both halves.
#[test]
fn probe_and_prime_refuse_the_same_receivers() {
    unsafe {
        for (i, refusal) in REFUSALS.iter().enumerate() {
            let obj = served_receiver(100.0 + i as f64);
            let token = stub_receiver_token(obj).unwrap();
            let name = format!("rsg{i}");
            let kb = key_bits(name.as_bytes());
            assert_eq!(read_stub_probe(token, kb), None, "premise: a fresh key");

            (refusal.apply)(obj);
            read_stub_prime(obj, kb, 0);
            (refusal.revert)(obj);
            assert_eq!(
                read_stub_probe(token, kb),
                None,
                "prime filed an entry under {}",
                refusal.name
            );

            read_stub_prime(obj, kb, 0);
            assert_eq!(
                read_stub_lookup(obj, kb),
                Some(100.0 + i as f64),
                "positive control for {}",
                refusal.name
            );

            (refusal.apply)(obj);
            let served = read_stub_lookup(obj, kb);
            (refusal.revert)(obj);
            assert_eq!(
                served, None,
                "lookup served a receiver under {}",
                refusal.name
            );
        }
    }
}
