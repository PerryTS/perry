//! `CreateDataPropertyOrThrow(O, key, value)` answered from the receiver
//! shape: the definition an object literal performs for each of its own
//! properties and a class performs for each public field.
//!
//! A definition never consults the prototype chain. On an ordinary,
//! extensible object it either creates `key` as a data property whose
//! attributes are all true, or, when `key` is already an own data property
//! with exactly those attributes, replaces its value. Both are facts of the
//! receiver ShapeId:
//!
//! 1. **Creation: the key-add edge.** The transition lattice holds an edge
//!    `(ShapeId, key) -> target` only for a shape that lacks `key`, so a hit
//!    is the creation, at the slot the target shape places the key. The edge
//!    is found by the ShapeId and the key's interned word, never by name.
//! 2. **Replacement: the own slot.** A key the shape already holds is
//!    overwritten in place when no own descriptor covers it.
//!
//! Anything else (no edge yet, an own accessor or non-default attributes for
//! the key, a non-extensible, exotic or dictionary receiver) is `None`, and
//! the caller runs the general definition, which also teaches the lattice the
//! edge for the next object of this shape.
//!
//! Nothing here runs user code. The only allocation is the store itself
//! (spill growth), and the stored value is handed back through the store's
//! own root.

use super::ObjectHeader;

/// Define `key = value` on `obj_value` from its shape (module docs), or
/// `None` when the shape does not answer; nothing was written then.
///
/// # Safety
/// `obj_value`, `key_value` and `value` are live values.
pub(crate) unsafe fn define_own_data_from_shape(
    obj_value: f64,
    key_value: f64,
    value: f64,
) -> Option<f64> {
    let bits = obj_value.to_bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_above_handle_band(addr) {
        return None;
    }
    let header = crate::value::addr_class::try_read_gc_header(addr)?;
    const BLOCKING: u16 = crate::gc::OBJ_FLAG_FROZEN
        | crate::gc::OBJ_FLAG_SEALED
        | crate::gc::OBJ_FLAG_NO_EXTEND
        | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved & BLOCKING != 0
    {
        return None;
    }
    let obj = addr as *mut ObjectHeader;
    // The word the lattice is keyed on: the interned string, found without
    // allocating (a pooled literal or an SSO immediate resolves to its
    // interned twin). Private names and canonical indices are refused there:
    // they are not ordinary named properties.
    let Some(key) = super::chain_store::interned_key_for_store(key_value) else {
        return define_listed_key_by_content(obj, key_value, value);
    };
    // 1. Creation. The chain-proven form is exactly the definition's append:
    // it vets the receiver kind, its flags and an own descriptor for the key,
    // and skips only the prototype-chain question a definition never asks.
    let mut refreshed: Option<(f64, f64, f64)> = None;
    if let Some(stored) = super::object_set_field_by_name_transition_chain_proven_value(
        obj,
        key,
        value,
        &mut refreshed,
    ) {
        return Some(stored);
    }
    // The key is interned, so the miss allocated nothing; the helper's
    // re-rooted copies are used whenever it hands them back all the same.
    let (obj_value, value) = match refreshed {
        Some((o, _, v)) => (o, v),
        None => (obj_value, value),
    };
    let obj = (obj_value.to_bits() & crate::value::POINTER_MASK) as *mut ObjectHeader;
    // 2. Replacement of an own data property with default attributes.
    if super::try_existing_own_data_overwrite(obj, key, value) {
        return Some(value);
    }
    None
}

/// Step 2 for a key whose content was never interned. No key-add edge names
/// such a key (the lattice is keyed by interned words, and the first append
/// of a key interns it), but the receiver's key list may already hold it: a
/// list holds its keys by content, and a class's declared fields are listed
/// under the module's literal strings, which are not interned. So the key is
/// found the way the list is keyed, by its bytes, and a default-attribute
/// data property is overwritten in place. `None` for anything else.
///
/// # Safety
/// `obj` is a live heap object; `key_value` and `value` are live values.
unsafe fn define_listed_key_by_content(
    obj: *mut ObjectHeader,
    key_value: f64,
    value: f64,
) -> Option<f64> {
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let bytes = crate::string::js_string_key_bytes(
        crate::value::JSValue::from_bits(key_value.to_bits()),
        &mut sso,
    )?;
    // Refused for the reason `interned_key_for_store` refuses them.
    match bytes.first() {
        Some(&first) if first != b'#' && !first.is_ascii_digit() => {}
        _ => return None,
    }
    // A key list that carries attributes must give this key none.
    if super::key_attrs::object_key_entry(obj, bytes) != 0 {
        return None;
    }
    super::try_existing_own_data_overwrite_by_content(obj, key_value, bytes, value).then_some(value)
}
