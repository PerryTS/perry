//! #8139: `toLocaleString` on an array / typed-array / buffer receiver.
//!
//! ## Why the tests go through `js_value_to_locale_string`
//!
//! That is the ONLY entry point the source-level zero-arg call reaches. The
//! HIR folds `x.toLocaleString()` on ANY receiver to `Expr::DateToLocaleString`
//! (`lower/expr_call/url_date_instance.rs`), and codegen lowers the
//! non-Number case to `js_value_to_locale_string`. The method-dispatch tower —
//! which has been answering correctly all along — is only reached by the
//! ARGUMENT-bearing spelling, which does not fold. Testing the tower would
//! therefore have passed before the fix.
//!
//! ## Why the assertions are exact strings, not "not [object …]"
//!
//! `join()` and `toLocaleString()` differ ONLY in digit grouping: node's
//! `new Int32Array([1234567, 2]).toLocaleString()` is `"1,234,567,2"` while
//! `.join()` is `"1234567,2"`. A test that only checked "did we stop saying
//! `[object Int32Array]`?" would pass against a `join` delegation, which is
//! the wrong answer and the one an obvious implementation reaches for. Every
//! expectation below was measured against node `26.5.1` (the `.node-version`
//! pin).

use crate::value::JSValue;

fn locale_string(receiver: f64) -> String {
    let result = crate::object::js_value_to_locale_string(receiver);
    let ptr = crate::value::js_get_string_pointer_unified(result) as *const crate::StringHeader;
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

fn boxed(addr: usize) -> f64 {
    f64::from_bits(JSValue::pointer(addr as *const u8).bits())
}

fn plain_array(values: &[f64]) -> f64 {
    let arr = crate::array::js_array_alloc(values.len() as u32);
    let mut arr = arr;
    for v in values {
        arr = crate::array::js_array_push_f64(arr, *v);
    }
    boxed(arr as usize)
}

fn typed(kind: u8, values: &[f64]) -> f64 {
    let ta = crate::typedarray::typed_array_alloc(kind, values.len() as u32);
    for (i, v) in values.iter().enumerate() {
        crate::typedarray::js_typed_array_set(ta, i as i32, *v);
    }
    boxed(ta as usize)
}

fn buffer(bytes: &[u8]) -> *mut crate::buffer::BufferHeader {
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

// ---------------------------------------------------------------------------

#[test]
fn a_plain_array_joins_its_elements_locale_strings() {
    // node: `[3,1,2].toLocaleString()` === "3,1,2".
    assert_eq!(locale_string(plain_array(&[3.0, 1.0, 2.0])), "3,1,2");
    // The grouping is the discriminator against a `join()` delegation.
    // node: `[1234567,2].toLocaleString()` === "1,234,567,2".
    assert_eq!(locale_string(plain_array(&[1234567.0, 2.0])), "1,234,567,2");
    // node: `[].toLocaleString()` === "".
    assert_eq!(locale_string(plain_array(&[])), "");
}

#[test]
fn a_typed_array_joins_its_elements_locale_strings() {
    // node: `new Int32Array([3,1,2]).toLocaleString()` === "3,1,2".
    assert_eq!(
        locale_string(typed(crate::typedarray::KIND_INT32, &[3.0, 1.0, 2.0])),
        "3,1,2"
    );
    // node: "1,234,567,2" — NOT `join()`'s "1234567,2".
    assert_eq!(
        locale_string(typed(crate::typedarray::KIND_INT32, &[1234567.0, 2.0])),
        "1,234,567,2"
    );
    // node: `new Float64Array([1.5, 1234567.25]).toLocaleString()` ===
    // "1.5,1,234,567.25".
    assert_eq!(
        locale_string(typed(crate::typedarray::KIND_FLOAT64, &[1.5, 1234567.25])),
        "1.5,1,234,567.25"
    );
}

#[test]
fn a_uint8array_joins_its_bytes_and_a_buffer_decodes_them() {
    // The Buffer-vs-`Uint8Array` split. Both are the same `BufferHeader` in
    // perry, and `Buffer.prototype.toLocaleString` is an OWN override that a
    // plain `Uint8Array` does not inherit.
    let u8a = buffer(&[3, 1, 2]) as usize;
    crate::buffer::mark_as_uint8array(u8a);
    // node: `new Uint8Array([3,1,2]).toLocaleString()` === "3,1,2".
    assert_eq!(locale_string(boxed(u8a)), "3,1,2");

    // node: `Buffer.from([104,105]).toLocaleString()` === "hi" (it delegates to
    // `toString()`, i.e. the utf8 decode). The bytes are printable so the
    // assertion can name the exact string.
    let buf = buffer(&[104, 105]) as usize;
    assert_eq!(locale_string(boxed(buf)), "hi");
}

#[test]
fn the_existing_receivers_are_unchanged() {
    // CONTROLS. The new arms sit above the `Object.prototype.toLocaleString`
    // tail, so these prove they declined every receiver that already worked.
    // node: 12345 → "12,345"; ({}) → "[object Object]"; "hi" → "hi";
    // true → "true".
    assert_eq!(locale_string(12345.0), "12,345");
    let obj = crate::object::js_object_alloc(0, 1);
    assert_eq!(locale_string(boxed(obj as usize)), "[object Object]");
    let s = crate::string::js_string_from_bytes(b"hi".as_ptr(), 2);
    assert_eq!(
        locale_string(f64::from_bits(JSValue::string_ptr(s).bits())),
        "hi"
    );
    assert_eq!(
        locale_string(f64::from_bits(crate::value::TAG_TRUE)),
        "true"
    );
}

#[test]
fn an_array_buffer_and_data_view_keep_the_object_tag() {
    // CONTROL + scope note. node: `new ArrayBuffer(2).toLocaleString()` ===
    // "[object ArrayBuffer]" (Object.prototype.toLocaleString → toString).
    // Perry renders "[object Uint8Array]" for both, which is a pre-existing
    // `Symbol.toStringTag` gap, NOT something the new buffer arm introduced —
    // the arm routes them to `dispatch_buffer_method`, whose `toLocaleString`
    // decodes, so the assertion here is that they are NOT served the decode.
    let ab = buffer(&[104, 105]) as usize;
    crate::buffer::mark_as_array_buffer(ab);
    assert_ne!(locale_string(boxed(ab)), "hi");
    let dv = buffer(&[104, 105]) as usize;
    crate::buffer::mark_as_data_view(dv);
    assert_ne!(locale_string(boxed(dv)), "hi");
}
