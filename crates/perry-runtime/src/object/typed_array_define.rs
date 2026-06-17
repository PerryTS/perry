//! Integer-Indexed exotic `[[DefineOwnProperty]]` for TypedArrays
//! (`Object.defineProperty` / `Reflect.defineProperty` on a typed array).
//!
//! Per the ECMAScript spec, a TypedArray is an Integer-Indexed exotic object:
//! defining a property whose key is a *CanonicalNumericIndexString* does NOT go
//! through the ordinary object machinery. Instead the index is validated
//! against the view's length and the descriptor is constrained to a writable,
//! enumerable, configurable data property; a valid in-bounds value descriptor
//! writes the element, and everything else is rejected (`Reflect.defineProperty`
//! returns `false`; `Object.defineProperty` throws a `TypeError`).
//!
//! Keys that are NOT canonical numeric index strings (`"length"`, `"foo"`,
//! `"1.0"`, `"+1"`, symbols, …) fall through to ordinary `[[DefineOwnProperty]]`
//! — this module reports `NotTypedArray` for those so the caller keeps its
//! existing behavior.
//!
//! Detached `ArrayBuffer`s are not modeled in Perry, so `IsValidIntegerIndex`
//! here never observes a detached backing store; the detached-buffer test262
//! cases remain out of scope.

use super::*;

/// Outcome of the TypedArray exotic `[[DefineOwnProperty]]` check.
pub(crate) enum TypedArrayDefineOutcome {
    /// Receiver isn't a TypedArray, or the key isn't a canonical numeric index
    /// — fall back to ordinary `[[DefineOwnProperty]]`.
    NotTypedArray,
    /// The integer-indexed branch rejected the definition (`false` / `TypeError`).
    Rejected,
    /// The integer-indexed branch accepted (and applied) the definition (`true`).
    Defined,
}

/// Resolve the raw heap address behind a NaN-boxed value (pointer tag or a bare
/// in-range pointer carried as raw bits).
#[inline]
fn value_addr(value: f64) -> usize {
    let bits = value.to_bits();
    if (bits >> 48) >= 0x7FF8 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        bits as usize
    }
}

/// If `value` is a TypedArray view, return `(addr, is_uint8_buffer, length)`.
/// `is_uint8_buffer` distinguishes the `BufferHeader`-backed `Uint8Array`
/// representation from the `TypedArrayHeader`-backed kinds.
fn typed_array_view_info(value: f64) -> Option<(usize, bool, u32)> {
    let addr = value_addr(value);
    if addr < 0x1000 {
        return None;
    }
    if crate::buffer::is_uint8array_buffer(addr) {
        let len = crate::buffer::js_buffer_length(addr as *const crate::buffer::BufferHeader);
        return Some((addr, true, len.max(0) as u32));
    }
    if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
        let len = crate::typedarray::js_typed_array_length(
            addr as *const crate::typedarray::TypedArrayHeader,
        );
        return Some((addr, false, len.max(0) as u32));
    }
    None
}

/// `CanonicalNumericIndexString(key)` — returns the numeric value when `key` is
/// the canonical string form of a Number (so `ToString(ToNumber(key)) == key`),
/// else `None`. `"-0"` is canonical (mapping to `-0`).
fn canonical_numeric_index(key: &str) -> Option<f64> {
    if key == "-0" {
        return Some(-0.0);
    }
    // ToNumber(String) — mirror the element-store string coercion: trim, then
    // parse as an IEEE double (parse failure → NaN).
    let n = key.trim().parse::<f64>().unwrap_or(f64::NAN);
    // Round-trip through JS `ToString(Number)` to confirm canonicality.
    let rendered = number_to_js_string(n);
    if rendered.as_deref() == Some(key) {
        Some(n)
    } else {
        None
    }
}

/// `ToString(Number)` rendered to an owned Rust string via the runtime's
/// number formatter (so `1e+21`, `NaN`, `Infinity` match JS output exactly).
fn number_to_js_string(n: f64) -> Option<String> {
    let s = crate::string::js_number_to_string(n);
    if s.is_null() {
        return None;
    }
    unsafe {
        let len = (*s).byte_len as usize;
        let data = (s as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        std::str::from_utf8(std::slice::from_raw_parts(data, len))
            .ok()
            .map(|s| s.to_string())
    }
}

/// `IsValidIntegerIndex(O, index)` minus the detached-buffer check (not modeled):
/// integral, not `-0`, and in `[0, length)`.
fn is_valid_integer_index(index: f64, length: u32) -> bool {
    if !index.is_finite() || index.fract() != 0.0 {
        return false;
    }
    if index == 0.0 && index.is_sign_negative() {
        return false; // -0
    }
    index >= 0.0 && index < length as f64
}

/// Is the descriptor's `name` field present and falsy (`{ name: false }`)?
unsafe fn field_present_and_false(desc: *mut ObjectHeader, name: &[u8]) -> bool {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    if !own_key_present(desc, key) {
        return false;
    }
    let v = js_object_get_field_by_name(desc as *const ObjectHeader, key);
    crate::value::js_is_truthy(f64::from_bits(v.bits())) == 0
}

/// Is the descriptor's `name` field present (regardless of value)?
unsafe fn field_present(desc: *mut ObjectHeader, name: &[u8]) -> bool {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    own_key_present(desc, key)
}

/// If `key_value` is a String key that is a CanonicalNumericIndexString, return
/// its numeric value. Returns `None` for symbols, non-string keys, and strings
/// that aren't canonical numeric indices (those go through ordinary semantics).
unsafe fn canonical_index_for_key(key_value: f64) -> Option<f64> {
    if crate::symbol::js_is_symbol(key_value) != 0 {
        return None;
    }
    let key_str_ptr = crate::builtins::js_string_coerce(key_value);
    if key_str_ptr.is_null() {
        return None;
    }
    let name_ptr = (key_str_ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let name_len = (*key_str_ptr).byte_len as usize;
    let key = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)).ok()?;
    canonical_numeric_index(key)
}

/// Result of a TypedArray Integer-Indexed `[[GetOwnProperty]]`.
pub(crate) enum TypedArrayOwnIndex {
    /// Not a TypedArray, or the key isn't a canonical numeric index — fall back
    /// to ordinary `[[GetOwnProperty]]`.
    NotTypedArray,
    /// Canonical numeric index but out of bounds / invalid — no own property.
    OutOfBounds,
    /// Valid in-bounds index — the element value (a NaN-boxed Number).
    Element(f64),
}

/// TypedArray Integer-Indexed `[[GetOwnProperty]]`: resolves a canonical numeric
/// index key to the element value (so `getOwnPropertyDescriptor(ta, "0")` yields
/// a `{ value, writable, enumerable, configurable: true }` data descriptor).
pub(crate) unsafe fn typed_array_own_index(obj_value: f64, key_value: f64) -> TypedArrayOwnIndex {
    let Some((addr, is_buf, length)) = typed_array_view_info(obj_value) else {
        return TypedArrayOwnIndex::NotTypedArray;
    };
    let Some(numeric_index) = canonical_index_for_key(key_value) else {
        return TypedArrayOwnIndex::NotTypedArray;
    };
    if !is_valid_integer_index(numeric_index, length) {
        return TypedArrayOwnIndex::OutOfBounds;
    }
    let idx = numeric_index as i32;
    let value = if is_buf {
        crate::buffer::js_buffer_get(addr as *const crate::buffer::BufferHeader, idx) as f64
    } else {
        crate::typedarray::js_typed_array_get(
            addr as *const crate::typedarray::TypedArrayHeader,
            idx,
        )
    };
    TypedArrayOwnIndex::Element(value)
}

/// TypedArray Integer-Indexed `[[DefineOwnProperty]]`. See module docs.
///
/// `key_value` and `descriptor_value` are the raw NaN-boxed arguments. The
/// caller must have already validated that `descriptor_value` is an object.
pub(crate) unsafe fn typed_array_define_own_property(
    obj_value: f64,
    key_value: f64,
    descriptor_value: f64,
) -> TypedArrayDefineOutcome {
    let Some((addr, is_buf, length)) = typed_array_view_info(obj_value) else {
        return TypedArrayDefineOutcome::NotTypedArray;
    };

    let Some(numeric_index) = canonical_index_for_key(key_value) else {
        // Symbol / non-string / non-canonical key → ordinary define handles it.
        return TypedArrayDefineOutcome::NotTypedArray;
    };

    // Canonical numeric index → integer-indexed branch. From here every path
    // returns Rejected or Defined; we never fall back to ordinary define.
    if !is_valid_integer_index(numeric_index, length) {
        return TypedArrayDefineOutcome::Rejected;
    }

    let desc = extract_obj_ptr(descriptor_value);
    if desc.is_null() {
        // Descriptor isn't ObjectHeader-backed; nothing constrains the index, so
        // accept with no element write (matches an all-default data descriptor).
        return TypedArrayDefineOutcome::Defined;
    }

    // A valid integer index requires a configurable, enumerable, writable data
    // descriptor. Any field that contradicts that rejects the definition.
    if field_present_and_false(desc, b"configurable") {
        return TypedArrayDefineOutcome::Rejected;
    }
    if field_present_and_false(desc, b"enumerable") {
        return TypedArrayDefineOutcome::Rejected;
    }
    if field_present(desc, b"get") || field_present(desc, b"set") {
        return TypedArrayDefineOutcome::Rejected; // accessor descriptor
    }
    if field_present_and_false(desc, b"writable") {
        return TypedArrayDefineOutcome::Rejected;
    }

    // If the descriptor carries a value, perform IntegerIndexedElementSet. The
    // ToNumber coercion may run user `valueOf` (and throw) before the write.
    let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
    if own_key_present(desc, value_key) {
        let value_field = js_object_get_field_by_name(desc as *const ObjectHeader, value_key);
        let value = f64::from_bits(value_field.bits());
        // Object values coerce via OrdinaryToPrimitive(number) first, running a
        // user `valueOf`/`toString` (which may throw). The resulting primitive is
        // fed to the per-kind element store, which re-coerces it numerically.
        let primitive = if crate::value::JSValue::from_bits(value.to_bits()).is_pointer() {
            match crate::value::ordinary_to_primitive_number_for_add(value) {
                crate::value::OrdinaryToPrimitiveOutcome::Primitive(p) => p,
                _ => f64::NAN,
            }
        } else {
            value
        };
        let idx = numeric_index as u32;
        if is_buf {
            let n = crate::value::JSValue::from_bits(primitive.to_bits()).to_number();
            crate::buffer::js_buffer_set(
                addr as *mut crate::buffer::BufferHeader,
                idx as i32,
                n as i32,
            );
        } else {
            crate::typedarray::js_typed_array_set(
                addr as *mut crate::typedarray::TypedArrayHeader,
                idx as i32,
                primitive,
            );
        }
    }

    TypedArrayDefineOutcome::Defined
}
