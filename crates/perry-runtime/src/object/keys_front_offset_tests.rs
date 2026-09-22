//! #10939: an ordered keys array's elements do not start at `header + 8`.
//!
//! `array_front_offset` is `array_physical_capacity - capacity`, so logical
//! element zero sits past the header for any array whose front has been
//! consumed — a dense-queue shift, a `GC_ARRAY_NAMED_PROPS` reserve, #9019's
//! reserved-floor seed, or a size-class round-up on its own. Four sites
//! hand-computed the offset instead of asking `keys_array_dense_slots` /
//! `array_elements_ptr`, and all four sit on a clone-before-mutate path for a
//! keys array.
//!
//! This pins the `[[Set]]` growth site end to end, which is the one a plain
//! program reaches. The failure is worse than losing a key: the clone's
//! published prefix is a region the collector walks as heap pointers, so
//! copying from the wrong base hands it `ArrayHeader` and reserve words to
//! trace — a missing property now, a SIGSEGV inside an unrelated collection
//! later.

use super::{js_object_alloc, js_object_set_field_by_name, object_keys_array};

/// The receiver's ordered key list, decoded.
unsafe fn key_names(obj: *mut super::ObjectHeader) -> Vec<String> {
    let keys = object_keys_array(obj);
    let (slots, len) = super::keys_array_dense_slots(keys);
    if slots.is_null() {
        return Vec::new();
    }
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let mut out = Vec::new();
    for i in 0..len {
        let value = crate::JSValue::from_bits((*slots.add(i)).to_bits());
        if let Some(bytes) = crate::string::js_string_key_bytes(value, &mut sso) {
            out.push(String::from_utf8_lossy(bytes).into_owned());
        }
    }
    out
}

unsafe fn push_name(
    keys: *mut crate::array::ArrayHeader,
    name: &str,
) -> *mut crate::array::ArrayHeader {
    let s = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::array::js_array_push(keys, crate::JSValue::string_ptr(s))
}

/// A keys array whose FRONT has been consumed must survive the
/// clone-before-push that a `[[Set]]` append performs on a shared key list.
#[test]
fn a_keys_array_with_a_consumed_front_survives_the_clone_before_push() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        // A real runtime path produces the offset: `shift_dense` hands the
        // vacated slot to the front offset instead of moving the survivors.
        // Building the header by hand would prove only that the hand-built
        // header is wrong.
        let mut keys = crate::array::js_array_alloc(4);
        for name in ["fo_dropped", "fo_a", "fo_b", "fo_c"] {
            keys = push_name(keys, name);
        }
        crate::array::js_array_shift_f64(keys);

        assert_eq!(
            crate::array::js_array_length(keys),
            3,
            "premise: the shift consumed exactly one element"
        );
        let hand_computed =
            (keys as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as usize;
        let accessor =
            crate::array::array_elements_ptr(keys as *const crate::array::ArrayHeader) as usize;
        assert_ne!(
            accessor, hand_computed,
            "premise: the front must be consumed, or `header + 8` and the \
             element accessor name the same address and this test proves \
             nothing"
        );

        // Install it as a receiver's ordered key list, and mark it SHARED —
        // the bit the transition cache stamps, and the only thing that sends
        // the next append down the clone-before-push branch.
        let obj = js_object_alloc(0, 8);
        super::set_object_keys_array_with_live(obj, keys, 3);
        let keys_gc = (keys as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        (*keys_gc).gc_flags |= crate::gc::GC_FLAG_SHAPE_SHARED;
        assert_eq!(
            key_names(obj),
            vec!["fo_a", "fo_b", "fo_c"],
            "premise: the receiver starts with the three surviving keys"
        );

        // The append that clones.
        let added = crate::string::js_string_from_bytes(b"fo_d".as_ptr(), 4);
        js_object_set_field_by_name(obj, added, 4.0);

        assert_eq!(
            key_names(obj),
            vec!["fo_a", "fo_b", "fo_c", "fo_d"],
            "the clone copied from `header + 8` instead of the array's element \
             base: the consumed front slot came back as a key and the last \
             real key was dropped (#10939)"
        );
    }
}
