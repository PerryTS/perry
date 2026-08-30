//! #8149: an `ArrayBuffer` / `SharedArrayBuffer` / `DataView` is a registered
//! buffer that is NOT integer-indexed exotic.
//!
//! ## Why these tests assert VALUES, never predicates
//!
//! Every case below pins the exact answer node gives, measured against node
//! `26.5.1` (the `.node-version` pin). A `DataView`'s bytes are zero-filled, so
//! a probe that only asked "is `dv[0]` falsy?" would pass under the bug —
//! `0` and `undefined` are both falsy. The tests therefore compare the NaN-box
//! tag (`is_undefined`), not truthiness, and the store-side tests assert BOTH
//! halves: that the expando exists AND that the byte is still zero. A fix that
//! wrote the byte *and* recorded the expando would pass the first half alone.
//!
//! ## What each control proves
//!
//! * `..._buffer_receiver_...` cases keep a real `Buffer` / `Uint8Array` on the
//!   byte path. That is the population the new predicate must NOT capture, and
//!   the sabotage arm "decline every registered buffer" fails exactly here.
//! * the plain-array and plain-object cases prove the new arm did not divert a
//!   receiver that never was a buffer.

use crate::value::JSValue;

fn undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn boxed(addr: usize) -> f64 {
    f64::from_bits(JSValue::pointer(addr as *const u8).bits())
}

fn is_undefined(v: f64) -> bool {
    v.to_bits() == crate::value::TAG_UNDEFINED
}

/// A registered `BufferHeader` holding `bytes`, marked as `mark` dictates.
fn buffer_with(bytes: &[u8]) -> *mut crate::buffer::BufferHeader {
    let buf = crate::buffer::buffer_alloc(bytes.len() as u32);
    unsafe {
        (*buf).length = bytes.len() as u32;
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            crate::buffer::buffer_data_mut(buf),
            bytes.len(),
        );
    }
    buf
}

fn data_view(bytes: &[u8]) -> usize {
    let buf = buffer_with(bytes) as usize;
    crate::buffer::mark_as_data_view(buf);
    buf
}

fn array_buffer(bytes: &[u8]) -> usize {
    let buf = buffer_with(bytes) as usize;
    crate::buffer::mark_as_array_buffer(buf);
    buf
}

/// A node `Buffer`: registered, marked nothing. (`Buffer.from` does not call
/// `mark_as_uint8array` — that mark distinguishes the `Uint8Array` CONSTRUCTOR
/// path, which is why `Object.keys` was right for one and `[]` for the other.)
fn node_buffer(bytes: &[u8]) -> usize {
    buffer_with(bytes) as usize
}

fn string_keys(arr: *mut crate::array::ArrayHeader) -> Vec<String> {
    let n = crate::array::js_array_length(arr);
    (0..n)
        .filter_map(|i| {
            let v = crate::array::js_array_get(arr, i);
            let ptr = crate::value::js_get_string_pointer_unified(f64::from_bits(v.bits()))
                as *const crate::StringHeader;
            if ptr.is_null() {
                return None;
            }
            unsafe {
                let len = (*ptr).byte_len as usize;
                let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                std::str::from_utf8(std::slice::from_raw_parts(data, len))
                    .ok()
                    .map(str::to_owned)
            }
        })
        .collect()
}

fn key(name: &str) -> f64 {
    let s = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    f64::from_bits(JSValue::string_ptr(s).bits())
}

// ---------------------------------------------------------------------------
// The predicate itself
// ---------------------------------------------------------------------------

#[test]
fn only_array_buffer_and_data_view_are_non_indexed_views() {
    let dv = data_view(&[1, 2, 3, 4]);
    let ab = array_buffer(&[1, 2, 3, 4]);
    let b = node_buffer(&[1, 2, 3]);
    let u8a = node_buffer(&[1, 2, 3]);
    crate::buffer::mark_as_uint8array(u8a);

    assert!(crate::buffer::is_non_indexed_buffer_view(dv));
    assert!(crate::buffer::is_non_indexed_buffer_view(ab));
    assert!(!crate::buffer::is_non_indexed_buffer_view(b));
    assert!(!crate::buffer::is_non_indexed_buffer_view(u8a));

    assert!(!crate::buffer::is_byte_indexed_buffer(dv));
    assert!(!crate::buffer::is_byte_indexed_buffer(ab));
    assert!(crate::buffer::is_byte_indexed_buffer(b));
    assert!(crate::buffer::is_byte_indexed_buffer(u8a));

    assert!(crate::buffer::is_node_buffer(b));
    assert!(!crate::buffer::is_node_buffer(u8a));
    assert!(!crate::buffer::is_node_buffer(ab));
    assert!(!crate::buffer::is_node_buffer(dv));

    // A plain array is not a buffer at all — the two `false` answers above are
    // NOT the same answer as this one, which is why `is_non_indexed_buffer_view`
    // exists separately from `!is_byte_indexed_buffer`.
    let arr = crate::array::js_array_alloc(2);
    assert!(!crate::buffer::is_byte_indexed_buffer(arr as usize));
    assert!(!crate::buffer::is_non_indexed_buffer_view(arr as usize));
}

// ---------------------------------------------------------------------------
// Element READ — the three index funnels named in #8149
// ---------------------------------------------------------------------------

#[test]
fn a_data_view_index_read_is_undefined_through_every_index_funnel() {
    let dv = data_view(&[7, 8, 9, 10]);
    let recv = boxed(dv);
    // node: `new DataView(new ArrayBuffer(4))[0]` === undefined, for EVERY
    // in-bounds index. `0` (the byte) and `undefined` are both falsy, so the
    // assertion is on the NaN-box tag.
    for idx in 0..4u32 {
        let i = f64::from(idx);
        assert!(
            is_undefined(crate::value::js_dyn_index_get(recv, i)),
            "js_dyn_index_get answered a byte for dv[{idx}]"
        );
        assert!(
            is_undefined(crate::object::js_object_get_index_polymorphic(dv as i64, i)),
            "js_object_get_index_polymorphic answered a byte for dv[{idx}]"
        );
        assert!(
            is_undefined(
                crate::typed_feedback::js_typed_feedback_array_index_get_fallback_boxed(0, recv, i,)
            ),
            "the typed-feedback fallback answered a byte for dv[{idx}]"
        );
    }
}

#[test]
fn an_array_buffer_index_read_is_undefined() {
    let ab = array_buffer(&[7, 8]);
    let recv = boxed(ab);
    assert!(is_undefined(crate::value::js_dyn_index_get(recv, 0.0)));
    assert!(is_undefined(
        crate::object::js_object_get_index_polymorphic(ab as i64, 1.0)
    ));
}

#[test]
fn a_buffer_receiver_still_reads_its_bytes() {
    // CONTROL. `Buffer.from([1,2,3])[1]` is `2` in node, and the sabotage arm
    // "decline every registered buffer" fails here.
    let b = node_buffer(&[1, 2, 3]);
    let recv = boxed(b);
    assert_eq!(crate::value::js_dyn_index_get(recv, 1.0), 2.0);
    assert_eq!(
        crate::object::js_object_get_index_polymorphic(b as i64, 2.0),
        3.0
    );
    assert_eq!(
        crate::typed_feedback::js_typed_feedback_array_index_get_fallback_boxed(0, recv, 0.0),
        1.0
    );
}

#[test]
fn a_plain_array_receiver_is_untouched_by_the_view_arm() {
    // CONTROL: the new question is asked of buffers only.
    let arr = crate::array::js_array_alloc(3);
    for v in [10.0, 20.0, 30.0] {
        crate::array::js_array_push_f64(arr, v);
    }
    assert_eq!(
        crate::value::js_dyn_index_get(boxed(arr as usize), 1.0),
        20.0
    );
    assert_eq!(
        crate::object::js_object_get_index_polymorphic(arr as i64, 2.0),
        30.0
    );
}

// ---------------------------------------------------------------------------
// Element STORE — an ordinary own property, not a byte
// ---------------------------------------------------------------------------

#[test]
fn a_data_view_index_store_creates_an_own_property_and_leaves_the_byte() {
    let dv = data_view(&[0, 0, 0, 0]);
    // node: `dv[0] = 7` → `dv[0] === 7`, `new Uint8Array(ab)[0] === 0`.
    crate::object::js_object_set_index_polymorphic(dv as i64, 0.0, 7.0);
    // BOTH halves. A fix that recorded the expando AND wrote the byte passes
    // the first assertion alone.
    assert_eq!(
        crate::buffer::buffer_get_own_prop(dv, "0"),
        Some(7.0),
        "the store must land as the own property \"0\""
    );
    assert_eq!(
        crate::buffer::js_buffer_get(dv as *const crate::buffer::BufferHeader, 0),
        0,
        "the store must NOT have written byte 0"
    );
    assert_eq!(crate::value::js_dyn_index_get(boxed(dv), 0.0), 7.0);
}

#[test]
fn a_data_view_index_store_through_dyn_index_set_creates_an_own_property() {
    let dv = data_view(&[0, 0]);
    crate::value::js_dyn_index_set(boxed(dv), 1.0, 5.0);
    assert_eq!(crate::buffer::buffer_get_own_prop(dv, "1"), Some(5.0));
    assert_eq!(
        crate::buffer::js_buffer_get(dv as *const crate::buffer::BufferHeader, 1),
        0
    );
}

#[test]
fn a_buffer_index_store_still_writes_the_byte() {
    // CONTROL. `Buffer.from([1,2,3])[0] = 9` writes the byte in node.
    let b = node_buffer(&[1, 2, 3]);
    crate::object::js_object_set_index_polymorphic(b as i64, 0.0, 9.0);
    assert_eq!(
        crate::buffer::js_buffer_get(b as *const crate::buffer::BufferHeader, 0),
        9
    );
    assert_eq!(crate::buffer::buffer_get_own_prop(b, "0"), None);
}

// ---------------------------------------------------------------------------
// `in` / `hasOwnProperty` / `.length`
// ---------------------------------------------------------------------------

#[test]
fn in_and_has_own_are_false_for_a_data_view_index() {
    let dv = data_view(&[1, 2, 3, 4]);
    let recv = boxed(dv);
    // node: `0 in dv` === false, `Object.prototype.hasOwnProperty.call(dv,"0")`
    // === false.
    assert_eq!(
        crate::object::js_object_has_property(recv, 0.0).to_bits(),
        crate::value::TAG_FALSE
    );
    assert_eq!(
        crate::object::js_object_has_property(recv, key("0")).to_bits(),
        crate::value::TAG_FALSE
    );
    assert_eq!(
        crate::object::js_object_has_own(recv, key("0")).to_bits(),
        crate::value::TAG_FALSE
    );
    // `length` is a %TypedArray% slot; a DataView has only `byteLength`.
    assert_eq!(
        crate::object::js_object_has_property(recv, key("length")).to_bits(),
        crate::value::TAG_FALSE
    );

    // After a store the index IS an own property — node agrees.
    crate::object::js_object_set_index_polymorphic(dv as i64, 0.0, 7.0);
    assert_eq!(
        crate::object::js_object_has_property(recv, 0.0).to_bits(),
        crate::value::TAG_TRUE
    );
    assert_eq!(
        crate::object::js_object_has_own(recv, key("0")).to_bits(),
        crate::value::TAG_TRUE
    );
}

#[test]
fn in_and_has_own_still_answer_true_for_a_buffer_index() {
    // CONTROL.
    let b = node_buffer(&[1, 2, 3]);
    let recv = boxed(b);
    assert_eq!(
        crate::object::js_object_has_property(recv, 1.0).to_bits(),
        crate::value::TAG_TRUE
    );
    assert_eq!(
        crate::object::js_object_has_own(recv, key("1")).to_bits(),
        crate::value::TAG_TRUE
    );
    assert_eq!(
        crate::object::js_object_has_property(recv, 3.0).to_bits(),
        crate::value::TAG_FALSE,
        "an out-of-bounds index is still absent"
    );
    assert_eq!(
        crate::object::js_object_has_property(recv, key("length")).to_bits(),
        crate::value::TAG_TRUE
    );
}

#[test]
fn length_is_undefined_for_a_view_and_the_byte_count_for_a_buffer() {
    let dv = data_view(&[1, 2, 3, 4]);
    let ab = array_buffer(&[1, 2, 3, 4, 5]);
    let b = node_buffer(&[1, 2, 3]);
    unsafe {
        // node: `dv.length` / `ab.length` === undefined; `byteLength` is the
        // count. `0` would ALSO be wrong-but-falsy, hence the tag assertion.
        assert!(is_undefined(crate::value::js_dynamic_object_get_property(
            boxed(dv),
            b"length".as_ptr() as *const i8,
            6
        )));
        assert!(is_undefined(crate::value::js_dynamic_object_get_property(
            boxed(ab),
            b"length".as_ptr() as *const i8,
            6
        )));
        assert_eq!(
            crate::value::js_dynamic_object_get_property(
                boxed(dv),
                b"byteLength".as_ptr() as *const i8,
                10
            ),
            4.0
        );
        // CONTROL: a Buffer keeps both spellings.
        assert_eq!(
            crate::value::js_dynamic_object_get_property(
                boxed(b),
                b"length".as_ptr() as *const i8,
                6
            ),
            3.0
        );
    }
}

// ---------------------------------------------------------------------------
// Enumeration — `Object.keys` / `.values` / `.entries` / `getOwnPropertyNames`
//
// This is also the memory-safety half: before the buffer arm existed these
// walked a `BufferHeader` as an `ObjectHeader`, reading payload bytes as
// `keys_array`. `Object.keys(new DataView(new ArrayBuffer(8)))` SIGBUS'd in
// `js_array_length` in any program that had also allocated a `Buffer` (exit
// 138); it answered `[]` only when those bytes happened to be zero.
// ---------------------------------------------------------------------------

#[test]
fn object_keys_of_a_buffer_lists_its_byte_indices() {
    let b = node_buffer(&[1, 2, 3]);
    // node: `Object.keys(Buffer.from([1,2,3]))` === ["0","1","2"]. Perry
    // answered `[]`.
    assert_eq!(
        string_keys(crate::object::js_object_keys_value(boxed(b))),
        vec!["0", "1", "2"]
    );
    let values = crate::object::js_object_values_value(boxed(b));
    let n = crate::array::js_array_length(values);
    let got: Vec<f64> = (0..n)
        .map(|i| f64::from_bits(crate::array::js_array_get(values, i).bits()))
        .collect();
    assert_eq!(got, vec![1.0, 2.0, 3.0]);
}

#[test]
fn object_keys_of_a_data_view_is_empty_then_reports_the_expando() {
    let dv = data_view(&[1, 2, 3, 4, 5, 6, 7, 8]);
    // node: `Object.keys(dv)` === []; after `dv[0] = 7` it is ["0"].
    assert!(string_keys(crate::object::js_object_keys_value(boxed(dv))).is_empty());
    crate::object::js_object_set_index_polymorphic(dv as i64, 0.0, 7.0);
    assert_eq!(
        string_keys(crate::object::js_object_keys_value(boxed(dv))),
        vec!["0"]
    );
}

/// The memory-safety half, with the poison the field measurement supplied.
///
/// The pre-fix walk read the `BufferHeader`'s PAYLOAD as
/// `ObjectHeader.keys_array` and then called `js_array_length` on it. It
/// answered `[]` whenever those bytes happened to be zero — which is what made
/// `Object.keys(Buffer)` look like a mere wrong answer — and SIGBUS'd when they
/// did not. The `.ts` repro was
/// `new ArrayBuffer(8); new DataView(ab); Buffer.from([1,2,3]);
/// Object.keys(D)`, exit 138, `js_array_length` ← `js_object_keys`.
///
/// Filling the payload with a non-zero, non-heap word makes the hazard
/// deterministic instead of allocation-order-dependent: with the arm removed
/// this reads that word as a pointer. The assertion is still on the ANSWER —
/// the poison is a stressor, not the subject.
#[test]
fn object_keys_of_a_poison_filled_view_neither_reads_nor_crashes() {
    let dv = data_view(&[0xAA; 64]);
    assert!(string_keys(crate::object::js_object_keys_value(boxed(dv))).is_empty());
    let names = crate::object::js_object_get_own_property_names(boxed(dv));
    let arr = (names.to_bits() & crate::value::POINTER_MASK) as *mut crate::array::ArrayHeader;
    assert!(string_keys(arr).is_empty());

    let b = node_buffer(&[0xAA; 3]);
    assert_eq!(
        string_keys(crate::object::js_object_keys_value(boxed(b))),
        vec!["0", "1", "2"]
    );
}

#[test]
fn get_own_property_names_of_a_buffer_lists_its_byte_indices() {
    let b = node_buffer(&[4, 5]);
    let names = crate::object::js_object_get_own_property_names(boxed(b));
    let arr = (names.to_bits() & crate::value::POINTER_MASK) as *mut crate::array::ArrayHeader;
    assert_eq!(string_keys(arr), vec!["0", "1"]);

    let ab = array_buffer(&[4, 5]);
    let names = crate::object::js_object_get_own_property_names(boxed(ab));
    let arr = (names.to_bits() & crate::value::POINTER_MASK) as *mut crate::array::ArrayHeader;
    assert!(string_keys(arr).is_empty());
}

#[test]
fn object_keys_of_a_plain_object_and_array_are_unchanged() {
    // CONTROL: the buffer arm sits at the very top of all three walks, so this
    // is what proves it declined a receiver that is not a buffer.
    let arr = crate::array::js_array_alloc(2);
    crate::array::js_array_push_f64(arr, 1.0);
    crate::array::js_array_push_f64(arr, 2.0);
    assert_eq!(
        string_keys(crate::object::js_object_keys_value(boxed(arr as usize))),
        vec!["0", "1"]
    );

    let obj = crate::object::js_object_alloc(0, 2);
    let k = crate::string::js_string_from_bytes(b"a".as_ptr(), 1);
    crate::object::js_object_set_field_by_name(obj, k, 1.0);
    assert_eq!(
        string_keys(crate::object::js_object_keys_value(boxed(obj as usize))),
        vec!["a"]
    );
}

// ---------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------

#[test]
fn json_stringify_of_a_view_is_an_empty_object_not_buffer_bytes() {
    let dv = data_view(&[1, 2, 3, 4]);
    let ab = array_buffer(&[9, 9]);
    // node: `JSON.stringify(new DataView(new ArrayBuffer(4)))` === "{}".
    // Perry emitted `{"type":"Buffer","data":[1,2,3,4]}` — a shape node never
    // produces here, and one that leaks the backing bytes.
    assert_eq!(json_of(boxed(dv)), "{}");
    assert_eq!(json_of(boxed(ab)), "{}");

    // CONTROL: a real Buffer keeps `Buffer.prototype.toJSON`'s shape.
    let b = node_buffer(&[1, 2]);
    assert_eq!(json_of(boxed(b)), r#"{"type":"Buffer","data":[1,2]}"#);
}

fn json_of(value: f64) -> String {
    let bits =
        unsafe { crate::json::js_json_stringify_full(value, undefined(), undefined()) } as u64;
    let ptr = crate::value::js_get_string_pointer_unified(f64::from_bits(bits))
        as *const crate::StringHeader;
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}
