//! #8113 error-tag / class-id collision tests (split out of `object/tests.rs`
//! to keep it under the 2,000-line cap, #10750).

use super::*;

// ---------------------------------------------------------------------------
// #8113 — the trap this header shrink had to disarm.
//
// `ObjectHeader` used to open with `object_type: u32`, prefix-punned against
// `error::ErrorHeader`'s first word, and NINE sites read raw offset 0 to answer
// "is this an Error?". Deleting the word makes offset 0 `class_id` — and
// `OBJECT_TYPE_ERROR` is **2**, while class ids are handed out from 1, densely,
// in source-declaration order. So a surviving raw read reclassifies every
// instance of the SECOND class a program declares as an `ErrorHeader` and reads
// `message`/`name`/`stack`/`errors` out of its field slots: a silent wrong
// answer of exactly the #8100 shape.
//
// These tests are SABOTAGE-SHAPED. Each first asserts that the confusable value
// really is sitting at offset 0 — so a green run proves the GcHeader-kind test
// fired, not that the fixture happened to look harmless.
// ---------------------------------------------------------------------------

/// The premise: an ordinary object CAN carry `class_id == OBJECT_TYPE_ERROR`,
/// and that value really is the first word of its header.
#[test]
fn an_ordinary_object_can_carry_the_error_type_tag_as_its_class_id() {
    let obj = js_object_alloc(crate::error::OBJECT_TYPE_ERROR, 2);
    assert!(!obj.is_null());
    unsafe {
        assert_eq!((*obj).class_id, crate::error::OBJECT_TYPE_ERROR);
        // Offset 0, read the way the retired discriminators read it.
        let raw_word_0 = std::ptr::read(obj as *const u32);
        assert_eq!(
            raw_word_0,
            crate::error::OBJECT_TYPE_ERROR,
            "test premise: the pre-#8113 raw offset-0 read now yields \
             OBJECT_TYPE_ERROR for an ordinary object"
        );
    }
}

/// `Error.isError()` must not be fooled by it. (`error.rs:750`.)
#[test]
fn error_is_error_rejects_an_object_whose_class_id_equals_the_error_tag() {
    let obj = js_object_alloc(crate::error::OBJECT_TYPE_ERROR, 2);
    let value = crate::value::js_nanbox_pointer(obj as i64);
    assert_eq!(
        crate::error::js_error_is_error(value).to_bits(),
        crate::value::TAG_FALSE,
        "class_id == OBJECT_TYPE_ERROR must not read as a native Error"
    );

    // Not over-narrowed: a real Error still answers true.
    let real = crate::error::js_error_new_with_message(crate::string::js_string_from_bytes(
        b"boom".as_ptr(),
        4,
    ));
    let real_value = crate::value::js_nanbox_pointer(real as i64);
    assert_eq!(
        crate::error::js_error_is_error(real_value).to_bits(),
        crate::value::TAG_TRUE,
        "a genuine ErrorHeader must still classify as an Error"
    );
}

/// `js_error_get_errors` must resolve `.errors` GENERICALLY for it rather than
/// returning the fixed `ErrorHeader.errors` slot. (`error.rs:1542`; the doc
/// there records the for-of corruption the fixed-slot read caused.)
#[test]
fn error_get_errors_does_not_read_a_fixed_slot_off_a_colliding_class_id() {
    let obj = js_object_alloc(crate::error::OBJECT_TYPE_ERROR, 2);
    unsafe {
        assert_eq!((*obj).class_id, crate::error::OBJECT_TYPE_ERROR);
        // Poison the slot the ErrorHeader layout would call `errors`.
        let key = crate::string::js_string_from_bytes(b"errors".as_ptr(), 6);
        let arr = crate::array::js_array_alloc(1);
        crate::object::js_object_set_field_by_name(
            obj,
            key,
            f64::from_bits(crate::value::js_nanbox_pointer(arr as i64).to_bits()),
        );
        let got = crate::error::js_error_get_errors(obj as *mut crate::error::ErrorHeader);
        assert_eq!(
            got as usize, arr as usize,
            "`.errors` on a class_id == 2 object must resolve as an ordinary \
             own property, not as ErrorHeader's fixed slot"
        );
    }
}

/// `js_dynamic_object_keys` must return the object's real keys, not the Error
/// triple. (`value/dynamic_object.rs:728`.)
#[test]
fn dynamic_object_keys_are_not_the_error_triple_for_a_colliding_class_id() {
    let obj = js_object_alloc(crate::error::OBJECT_TYPE_ERROR, 2);
    unsafe {
        let key = crate::string::js_string_from_bytes(b"kk8113".as_ptr(), 6);
        crate::object::js_object_set_field_by_name(obj, key, 1.0);
        let keys = crate::value::js_dynamic_object_keys(obj as i64);
        assert_eq!(
            crate::array::js_array_length(keys),
            1,
            "a class_id == 2 object must enumerate its OWN keys, not \
             [message, name, stack]"
        );
    }
}

/// The #6595 half: the store-plan gate must stay FALSE for a heap class object.
/// `object_is_regular` is the replacement for the deleted
/// `object_type == OBJECT_TYPE_REGULAR` read at `proxy.rs:1523`, and it is only
/// a valid one because it means `descriptor.object_kind == Ordinary` — not the
/// weaker "is an ObjectHeader".
#[test]
fn object_is_regular_excludes_a_heap_class_object() {
    let obj = js_object_alloc(0x8113_0001, 1);
    unsafe {
        assert!(
            crate::object::object_is_regular(obj),
            "a fresh ordinary object is regular"
        );
        crate::object::class_registry::js_object_mark_class(obj as i64);
        assert!(
            !crate::object::object_is_regular(obj),
            "#6595: a heap class object must NOT be 'regular' — the store-plan \
             gate at proxy.rs keys off exactly this"
        );
    }
}
