//! Virtual character indices for String exotic objects (#9810).
//!
//! Boxing stores the primitive and the fixed `length` property. Indices never
//! enter shapes or descriptor_state: a call with an unused string receiver
//! must not allocate one field, descriptor, and character per code unit.

use super::{ObjectHeader, PropertyAttrs};
use crate::{ArrayHeader, JSValue, StringHeader};

/// Consult only: no allocation, string coercion, or user code. Heap strings
/// already cache their UTF-16 length, so probes are independent of that length.
pub(super) fn length(owner: usize) -> Option<u32> {
    unsafe {
        let header = crate::value::addr_class::try_read_gc_header(owner)?;
        if header.obj_type != crate::gc::GC_TYPE_OBJECT
            || (*(owner as *const ObjectHeader)).class_id != 0xFFFF_00D1
        {
            return None;
        }
        let (_, payload) = crate::builtins::boxed_primitive_payload(
            crate::value::js_nanbox_pointer(owner as i64),
        )?;
        let value = JSValue::from_bits(payload.to_bits());
        if value.is_string() {
            let ptr = (value.bits() & crate::value::POINTER_MASK) as *const StringHeader;
            return Some(crate::string::js_string_length(ptr));
        }
        let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
        let (ptr, len) = crate::string::str_bytes_from_jsvalue(payload, &mut scratch)?;
        Some(crate::string::compute_utf16_len(ptr, len))
    }
}

pub(super) fn has_index(owner: usize, name: &str) -> bool {
    // Reject ordinary property names before probing the receiver metadata.
    let Some(index) = super::canonical_array_index(name) else {
        return false;
    };
    length(owner).is_some_and(|len| index < len)
}

pub(super) unsafe fn has_index_key(owner: usize, key: *const StringHeader) -> bool {
    crate::string::header_str_checked(key).is_some_and(|name| has_index(owner, name))
}

pub(super) enum Enumeration {
    Keys,
    Values,
    Entries,
}

unsafe fn is_enumerable(obj: *const ObjectHeader, key: *const StringHeader) -> bool {
    super::own_key_present(obj as *mut ObjectHeader, key)
        && crate::string::header_str_checked(key).is_some_and(|name| {
            super::get_property_attrs(obj as usize, name)
                .unwrap_or(PropertyAttrs::new(true, true, true))
                .enumerable()
        })
}

/// EnumerableOwnProperties over virtual indices and ordinary expando keys.
/// Snapshot once, then recheck ownership/enumerability before each value read:
/// an expando getter can delete or hide a later property and can trigger GC.
pub(super) unsafe fn enumerate(
    obj: *const ObjectHeader,
    kind: Enumeration,
) -> Option<*mut ArrayHeader> {
    length(obj as usize)?;
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_raw_const_ptr(obj);
    let names = obj_h.with_const_ptr(|obj: *const ObjectHeader| {
        super::js_object_get_own_property_names(crate::value::js_nanbox_pointer(obj as i64))
    });
    let names_h = scope.root_nanbox_f64(names);
    let count = crate::array::js_array_length(crate::value::js_nanbox_get_pointer(
        names_h.get_nanbox_f64(),
    ) as *const ArrayHeader);
    let result = crate::array::js_array_alloc(count);
    let result_h = scope.root_raw_mut_ptr(result);
    for i in 0..count {
        let iter_scope = crate::gc::RuntimeHandleScope::new();
        let key = crate::array::js_array_get(
            crate::value::js_nanbox_get_pointer(names_h.get_nanbox_f64()) as *const ArrayHeader,
            i,
        );
        let key_h = iter_scope.root_nanbox_u64(key.bits());
        let key_ptr = crate::builtins::js_string_coerce(key_h.get_nanbox_f64());
        let key_ptr_h = iter_scope.root_string_ptr(key_ptr);
        let enumerable =
            obj_h.with_const_ptr(|obj| key_ptr_h.with_const_ptr(|key| is_enumerable(obj, key)));
        if !enumerable {
            continue;
        }
        let output = match kind {
            Enumeration::Keys => key_h.get_nanbox_u64(),
            Enumeration::Values | Enumeration::Entries => {
                let value = obj_h.with_const_ptr(|obj| {
                    key_ptr_h.with_const_ptr(|key| super::js_object_get_field_by_name(obj, key))
                });
                if matches!(kind, Enumeration::Values) {
                    value.bits()
                } else {
                    let value_h = iter_scope.root_nanbox_u64(value.bits());
                    let pair = crate::array::js_array_alloc(2);
                    let pair_h = iter_scope.root_raw_mut_ptr(pair);
                    pair_h.with_mut_ptr(|pair| {
                        crate::array::js_array_push_f64(pair, key_h.get_nanbox_f64())
                    });
                    pair_h.with_mut_ptr(|pair| {
                        crate::array::js_array_push_f64(pair, value_h.get_nanbox_f64())
                    });
                    pair_h.with_mut_ptr(|pair: *mut ArrayHeader| JSValue::array_ptr(pair).bits())
                }
            }
        };
        result_h
            .with_mut_ptr(|result| crate::array::js_array_push(result, JSValue::from_bits(output)));
    }
    Some(result_h.with_mut_ptr(|result| result))
}

/// The ordinary rest helper copies physical slots. String indices have no
/// slots, so read the included enumerable properties by name instead.
pub(super) unsafe fn rest(
    source: *const ObjectHeader,
    excluded: *const ArrayHeader,
) -> *mut ObjectHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let source_h = scope.root_raw_const_ptr(source);
    let excluded_h = scope.root_raw_const_ptr(excluded);
    let keys = enumerate(source, Enumeration::Keys).unwrap();
    let keys_h = scope.root_raw_const_ptr(keys);
    let result = super::js_object_alloc(0, 0);
    let result_h = scope.root_raw_mut_ptr(result);
    let count = keys_h.with_const_ptr(|keys| crate::array::js_array_length(keys));
    for i in 0..count {
        let iter_scope = crate::gc::RuntimeHandleScope::new();
        let key = keys_h.with_const_ptr(|keys| crate::array::js_array_get(keys, i));
        let key_h = iter_scope.root_nanbox_u64(key.bits());
        let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
        let bytes = crate::string::js_string_key_bytes(key, &mut scratch).unwrap();
        let skip = excluded_h.with_const_ptr(|excluded: *const ArrayHeader| {
            !excluded.is_null()
                && (0..crate::array::js_array_length(excluded)).any(|j| {
                    crate::string::js_string_key_matches_bytes(
                        crate::array::js_array_get(excluded, j),
                        bytes,
                    )
                })
        });
        if skip {
            continue;
        }
        let key_ptr = crate::builtins::js_string_coerce(key_h.get_nanbox_f64());
        let key_ptr_h = iter_scope.root_string_ptr(key_ptr);
        if !source_h
            .with_const_ptr(|source| key_ptr_h.with_const_ptr(|key| is_enumerable(source, key)))
        {
            continue;
        }
        let value = source_h.with_const_ptr(|source| {
            key_ptr_h.with_const_ptr(|key| super::js_object_get_field_by_name(source, key))
        });
        result_h.with_mut_ptr(|result| {
            key_ptr_h.with_const_ptr(|key| {
                super::js_object_set_field_by_name(result, key, f64::from_bits(value.bits()))
            })
        });
    }
    result_h.with_mut_ptr(|result| result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boxing_and_reflection_do_not_store_character_properties() {
        unsafe {
            let text = "a".repeat(4096);
            let string = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
            let boxed = crate::builtins::js_boxed_string_new(
                crate::value::js_nanbox_string(string as i64),
                1,
            );
            let scope = crate::gc::RuntimeHandleScope::new();
            let boxed_h = scope.root_nanbox_f64(boxed);
            let check_storage = || {
                let owner = crate::value::js_nanbox_get_pointer(boxed_h.get_nanbox_f64()) as usize;
                assert_eq!(length(owner), Some(4096));
                let physical_keys = crate::object::object_keys_array(owner as *const ObjectHeader);
                assert_eq!(
                    crate::array::js_array_length(physical_keys),
                    1,
                    "only length is stored"
                );
                assert_eq!(
                    crate::state::state()
                        .descriptors
                        .property_descriptors
                        .borrow()
                        .keys()
                        .filter(|(ptr, _)| *ptr == owner)
                        .count(),
                    1,
                    "indices must not populate descriptor_state",
                );
                assert!(has_index(owner, "4095"));
                assert!(!has_index(owner, "4096"));
                assert!(!has_index(owner, "01"));
                let attrs = crate::object::get_property_attrs(owner, "4095").unwrap();
                assert!(!attrs.writable() && attrs.enumerable() && !attrs.configurable());
            };
            check_storage();
            let keys = crate::object::js_object_keys_value(boxed_h.get_nanbox_f64());
            assert_eq!(crate::array::js_array_length(keys), 4096);
            check_storage();
            let key = crate::string::js_string_from_bytes(b"4095".as_ptr(), 4);
            let descriptor = crate::object::js_object_get_own_property_descriptor(
                boxed_h.get_nanbox_f64(),
                crate::value::js_nanbox_string(key as i64),
            );
            assert!(!JSValue::from_bits(descriptor.to_bits()).is_undefined());
            check_storage();
        }
    }
}
