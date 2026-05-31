//! Helpers for `Object.hasOwn` ToObject and primitive own-key handling.

pub(crate) fn throw_to_object_nullish_type_error() -> ! {
    let message = "Cannot convert undefined or null to object";
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

unsafe fn string_header_as_str<'a>(key: *const crate::StringHeader) -> Option<&'a str> {
    if key.is_null() {
        return None;
    }
    let len = (*key).byte_len as usize;
    let data = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    std::str::from_utf8(bytes).ok()
}

pub(super) unsafe fn string_primitive_own_key_present(
    value: f64,
    key: *const crate::StringHeader,
) -> bool {
    let Some(key_name) = string_header_as_str(key) else {
        return false;
    };
    if key_name == "length" {
        return true;
    }
    let Some(index) = super::canonical_array_index(key_name) else {
        return false;
    };
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some((ptr, blen)) = crate::string::str_bytes_from_jsvalue(value, &mut scratch) else {
        return false;
    };
    if ptr.is_null() {
        return false;
    }
    index < crate::string::compute_utf16_len(ptr, blen)
}

pub(super) unsafe fn array_own_key_present(
    arr: *const crate::array::ArrayHeader,
    key: *const crate::StringHeader,
) -> bool {
    let Some(key_name) = string_header_as_str(key) else {
        return false;
    };
    if key_name == "length" {
        return true;
    }
    let Some(index) = super::canonical_array_index(key_name) else {
        return false;
    };
    let length = (*arr).length;
    if index >= length {
        return false;
    }
    let elements =
        (arr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const u64;
    std::ptr::read(elements.add(index as usize)) != crate::value::TAG_HOLE
}
