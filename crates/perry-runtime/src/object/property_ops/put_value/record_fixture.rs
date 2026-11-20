//! Receivers for the store-site tests. Object model step 4: a `JSON.parse`
//! record carries an overflow from birth (§3.4, the order authority of a data
//! family), and store-site memos do not serve a receiver with an overflow
//! until step 5 replaces them with the unknown-receiver probe. A test of the
//! memo itself therefore takes a [`record`]: the same keys and values, born
//! as fixed slots with no overflow.
use crate::value::POINTER_MASK;

/// A `JSON.parse` result: class-less, ordinary, with an overflow from birth.
pub(crate) fn json(src: &[u8]) -> f64 {
    let text = crate::string::js_string_from_bytes(src.as_ptr(), src.len() as u32);
    let value = unsafe { crate::json::js_json_parse(text) };
    assert!(value.is_pointer(), "JSON.parse must yield an object");
    f64::from_bits(value.bits())
}

/// The flat JSON object `src` as a plain record: class-less, born marked
/// ordinary with its keys as fixed slots (Any lanes), sharing its ShapeId
/// with every record of the same key sequence, and with no overflow.
pub(crate) fn record(src: &[u8]) -> f64 {
    let source = json(src);
    let source_obj = (source.to_bits() & POINTER_MASK) as *mut crate::ObjectHeader;
    let mut names: Vec<Vec<u8>> = Vec::new();
    let mut values = Vec::new();
    unsafe {
        let keys = crate::object::js_object_keys(source_obj);
        for i in 0..crate::array::js_array_length(keys) {
            let key = crate::JSValue::from_bits(crate::array::js_array_get_f64(keys, i).to_bits());
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let name = crate::string::js_string_key_bytes(key, &mut scratch)
                .expect("a string key")
                .to_vec();
            let value = crate::object::js_object_get_field_by_name(source_obj, atom(&name));
            values.push(f64::from_bits(value.bits()));
            names.push(name);
        }
        let name_refs: Vec<&[u8]> = names.iter().map(Vec::as_slice).collect();
        let keys = crate::object::static_shapes::canonical_keys_for_names(&name_refs);
        let obj =
            crate::object::alloc_plain::alloc_plain_record_with_keys(names.len() as u32, keys);
        for (name, value) in names.iter().zip(values) {
            crate::object::js_object_set_field_by_name(obj, atom(name), value);
        }
        assert!(
            !crate::object::overflow::has_overflow(obj),
            "a record keeps every key in its layout"
        );
        f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits())
    }
}

fn atom(name: &[u8]) -> *const crate::StringHeader {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let hash = crate::object::key_bytes_hash(name.as_ptr(), name.len());
    crate::string::js_string_intern(key, hash)
}
