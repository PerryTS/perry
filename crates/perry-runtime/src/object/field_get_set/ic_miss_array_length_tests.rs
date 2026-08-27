//! Array-length fast-path IC-miss tests.
//!
//! Split out of `ic_miss.rs` to keep it under the 2,000-line file gate.

/// #7753: the `arr.length` short-circuit must answer EXACTLY what the full
/// ladder answers, for a fresh array, a grown one, and an empty one — and
/// must not fire for any other key on an array receiver, nor for `length`
/// on a non-array. Comparing against `js_object_get_field_by_name_f64` (the
/// path the read took before the short-circuit) is what makes this a
/// behaviour-equivalence test rather than a restatement of the fast path.
#[test]
fn array_length_short_circuit_agrees_with_the_full_ladder() {
    let _lock = crate::gc::global_side_table_test_lock();
    {
        let len_key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
        let other_key = crate::string::js_string_from_bytes(b"lengtx".as_ptr(), 6);
        for n in [0u32, 1, 5, 40] {
            let mut arr = crate::array::js_array_alloc(n.max(1));
            for i in 0..n {
                arr = crate::array::js_array_push(arr, crate::value::JSValue::number(i as f64));
            }
            let obj = arr as *const super::ObjectHeader;
            let mut cache = [0i64; super::PIC_CACHE_WORDS];
            let via_ic = super::js_object_get_field_ic_miss(obj, len_key, &mut cache);
            let via_ladder = super::js_object_get_field_by_name_f64(obj, len_key);
            assert_eq!(
                via_ic.to_bits(),
                via_ladder.to_bits(),
                "length disagreed for a {n}-element array"
            );
            assert_eq!(via_ic, n as f64, "length wrong for a {n}-element array");
            // A same-length key that is not `length` must not be captured
            // by the fast path.
            assert_eq!(
                super::js_object_get_field_ic_miss(obj, other_key, &mut cache).to_bits(),
                super::js_object_get_field_by_name_f64(obj, other_key).to_bits(),
                "a non-`length` key on an array must take the normal path"
            );
        }
        // `length` on a plain OBJECT must not be answered by the array
        // short-circuit — it is an ordinary (absent) property there.
        let plain = crate::object::js_object_alloc(0, 0);
        let mut cache = [0i64; super::PIC_CACHE_WORDS];
        assert_eq!(
            super::js_object_get_field_ic_miss(plain, len_key, &mut cache).to_bits(),
            super::js_object_get_field_by_name_f64(plain, len_key).to_bits(),
            "`length` on a plain object must keep its normal answer"
        );
    }
}

/// Array subclasses are ObjectHeader-backed, so a polymorphic loop over
/// differently shaped instances cannot use the real-Array short circuit
/// above or reliably stay in one property PIC. The dense subclass proof
/// must return the live own `length`, while unrelated object-backed values
/// continue through ordinary property lookup.
#[test]
fn array_subclass_length_short_circuit_preserves_object_semantics() {
    const CLASS_ID_ARRAY: u32 = 0xFFFF_0024;
    const SUBCLASS_ID: u32 = 0x0077_8655;
    let _lock = crate::gc::global_side_table_test_lock();
    crate::object::js_register_class_parent(SUBCLASS_ID, CLASS_ID_ARRAY);

    let obj = crate::object::js_object_alloc(SUBCLASS_ID, 2);
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    crate::node_stream::js_array_subclass_init(receiver, 0.0);
    for (index, value) in [11.0, 22.0, 33.0].into_iter().enumerate() {
        crate::object::js_object_set_index_polymorphic(obj as i64, index as f64, value);
    }

    let len_key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    let mut cache = [0i64; super::PIC_CACHE_WORDS];
    let via_ic = super::js_object_get_field_ic_miss(obj, len_key, &mut cache);
    let via_ladder = super::js_object_get_field_by_name_f64(obj, len_key);
    assert_eq!(via_ic.to_bits(), via_ladder.to_bits());
    assert_eq!(via_ic, 3.0, "the fast path must observe the live length");

    let plain = crate::object::js_object_alloc(0, 1);
    crate::object::js_object_set_field_by_name(plain, len_key, 123.0);
    let mut plain_cache = [0i64; super::PIC_CACHE_WORDS];
    assert_eq!(
        super::js_object_get_field_ic_miss(plain, len_key, &mut plain_cache),
        123.0,
        "ordinary objects must retain their own `length` property semantics"
    );
}
