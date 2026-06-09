//! Helpers for `Object.hasOwn` ToObject and primitive own-key handling.

/// #3655: is `key` an OWN property of the function/closure at `ptr`?
///
/// Every function carries built-in own data properties `name` and `length`
/// (`{ writable:false, enumerable:false, configurable:true }`); constructors
/// also carry `prototype`. These are synthesized from the name/arity
/// registries rather than stored as object fields, so the generic
/// `own_key_present` (which reads `ObjectHeader.keys_array`) can't see them —
/// and reading that offset off a closure is out-of-bounds. User-attached
/// props (`fn.x = 1`) live in the closure dynamic-prop side table. A `delete`d
/// slot (`closure_is_key_deleted`) is no longer own. Mirrors the value-read
/// closure arm in `field_get_set.rs`.
pub(crate) fn closure_own_key_present(ptr: usize, key: &str) -> bool {
    if crate::closure::closure_is_key_deleted(ptr, key) {
        return false;
    }
    if super::get_accessor_descriptor(ptr, key).is_some() {
        return true;
    }
    match key {
        // Always-present built-in function slots.
        "name" | "length" => true,
        // A constructor-capable function's `.prototype` is an own property
        // from birth even though Perry materializes the object lazily —
        // `f.hasOwnProperty('prototype')` must be true BEFORE any read of
        // `f.prototype`. The for-read helper materializes (idempotently) and
        // returns None for arrows/builtins, which really have no own slot.
        "prototype" => {
            crate::closure::closure_has_own_dynamic_prop(ptr, key) || {
                let val = crate::value::js_nanbox_pointer(ptr as i64);
                super::class_registry::ordinary_function_prototype_value_for_read(val).is_some()
            }
        }
        // User props are real own dynamic props in the side table.
        _ => crate::closure::closure_has_own_dynamic_prop(ptr, key),
    }
}

/// `RequireObjectCoercible(value)` for object destructuring binding/assignment
/// (`let {a} = src`, `method({a}) {}`). Throws a TypeError when `value` is
/// `null` or `undefined` (so even an empty pattern `{}` rejects nullish input),
/// otherwise returns the value unchanged. Property reads happen afterward via
/// the ordinary `[[Get]]` path, which boxes primitives as needed.
#[no_mangle]
pub extern "C" fn js_require_object_coercible(value: f64) -> f64 {
    let bits = value.to_bits();
    if bits == crate::value::TAG_UNDEFINED || bits == crate::value::TAG_NULL {
        throw_to_object_nullish_type_error();
    }
    value
}

pub(crate) fn throw_to_object_nullish_type_error() -> ! {
    let message = "Cannot convert undefined or null to object";
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// #3655: borrow a `StringHeader` key as `&str` (UTF-8 validated). Public
/// sibling of the private `string_header_as_str` for the closure-key callers
/// in `object_ops.rs` / `descriptors.rs`.
pub(crate) unsafe fn str_from_string_header<'a>(
    key: *const crate::StringHeader,
) -> Option<&'a str> {
    string_header_as_str(key)
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
    // Issue #233: resolve a grow forwarding pointer so `arr.hasOwnProperty(i)` /
    // `getOwnPropertyDescriptor` stay correct after `arr.length = N` reallocated
    // the buffer (the `in` path already does this; the named-method paths reach
    // here with the stale pre-grow pointer otherwise).
    let arr = crate::array::clean_arr_ptr(arr);
    let Some(key_name) = string_header_as_str(key) else {
        return false;
    };
    if key_name == "length" {
        return true;
    }
    if super::get_accessor_descriptor(arr as usize, key_name).is_some() {
        return true;
    }
    if crate::array::array_named_property_has(arr, key) {
        return true;
    }
    let Some(index) = super::canonical_array_index(key_name) else {
        return false;
    };
    let length = (*arr).length;
    if index >= length {
        return false;
    }
    if index >= (*arr).capacity {
        return false;
    }
    let elements =
        (arr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const u64;
    std::ptr::read(elements.add(index as usize)) != crate::value::TAG_HOLE
}

pub(super) unsafe fn buffer_own_key_present(
    buf: *const crate::buffer::BufferHeader,
    key: *const crate::StringHeader,
) -> bool {
    let Some(key_name) = string_header_as_str(key) else {
        return false;
    };
    let Some(index) = super::canonical_array_index(key_name) else {
        return false;
    };
    index < (*buf).length
}
