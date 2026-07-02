//! `Reflect.{define,get,has,delete}Metadata` and related metadata-keys
//! helpers, backed by the `REFLECT_METADATA` thread-local in the parent
//! module. Extracted from `proxy.rs` to keep that file under the 2000-line
//! cap; behavior is unchanged.

use std::collections::HashSet;

use super::{
    MetadataKey, POINTER_MASK, POINTER_TAG, REFLECT_METADATA, TAG_FALSE, TAG_NULL, TAG_TRUE,
    TAG_UNDEFINED,
};

#[no_mangle]
pub extern "C" fn js_reflect_define_metadata(
    key: f64,
    value: f64,
    target: f64,
    property_key: f64,
) -> f64 {
    if let Some(metadata_key) = make_metadata_key(key, target, property_key) {
        REFLECT_METADATA.with(|store| {
            store.borrow_mut().insert(metadata_key, value);
        });
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_reflect_get_metadata(key: f64, target: f64, property_key: f64) -> f64 {
    let Some(key_part) = metadata_key_part(key) else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    let Some(property_key_part) = metadata_property_key_part(property_key) else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    get_metadata_in_prototype_chain(&key_part, target, property_key_part.as_ref())
}

fn get_own_metadata(key: f64, target: f64, property_key: f64) -> f64 {
    let Some(metadata_key) = make_metadata_key(key, target, property_key) else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    REFLECT_METADATA.with(|store| {
        store
            .borrow()
            .get(&metadata_key)
            .copied()
            .unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED))
    })
}

#[no_mangle]
pub extern "C" fn js_reflect_get_own_metadata(key: f64, target: f64, property_key: f64) -> f64 {
    get_own_metadata(key, target, property_key)
}

#[no_mangle]
pub extern "C" fn js_reflect_has_metadata(key: f64, target: f64, property_key: f64) -> f64 {
    let Some(key_part) = metadata_key_part(key) else {
        return f64::from_bits(TAG_FALSE);
    };
    let Some(property_key_part) = metadata_property_key_part(property_key) else {
        return f64::from_bits(TAG_FALSE);
    };
    let found = get_metadata_in_prototype_chain(&key_part, target, property_key_part.as_ref())
        .to_bits()
        != TAG_UNDEFINED;
    f64::from_bits(if found { TAG_TRUE } else { TAG_FALSE })
}

#[no_mangle]
pub extern "C" fn js_reflect_has_own_metadata(key: f64, target: f64, property_key: f64) -> f64 {
    let Some(metadata_key) = make_metadata_key(key, target, property_key) else {
        return f64::from_bits(TAG_FALSE);
    };
    let found = REFLECT_METADATA.with(|store| store.borrow().contains_key(&metadata_key));
    f64::from_bits(if found { TAG_TRUE } else { TAG_FALSE })
}

#[no_mangle]
pub extern "C" fn js_reflect_get_metadata_keys(target: f64, property_key: f64) -> f64 {
    metadata_keys_for(target, property_key, true)
}

#[no_mangle]
pub extern "C" fn js_reflect_get_own_metadata_keys(target: f64, property_key: f64) -> f64 {
    metadata_keys_for(target, property_key, false)
}

#[no_mangle]
pub extern "C" fn js_reflect_delete_metadata(key: f64, target: f64, property_key: f64) -> f64 {
    let Some(metadata_key) = make_metadata_key(key, target, property_key) else {
        return f64::from_bits(TAG_FALSE);
    };
    let deleted = REFLECT_METADATA.with(|store| store.borrow_mut().remove(&metadata_key).is_some());
    f64::from_bits(if deleted { TAG_TRUE } else { TAG_FALSE })
}

/// Map a metadata `target` to a GC-stable key, folding a class's prototype onto
/// the class itself.
///
/// TypeScript's `emitDecoratorMetadata` stores instance-member `design:type` on
/// `Class.prototype`, and class-transformer reads it back with
/// `Reflect.getMetadata("design:type", SomeClass.prototype, prop)`
/// (TransformOperationExecutor). Perry instead emits that metadata against the
/// class constructor (`ClassRef`), and several existing tests
/// (`test_decorators_legacy_property_metadata.ts`) read it back off the class.
/// To satisfy BOTH access shapes we treat a class's prototype and its
/// constructor as one metadata bucket: any class-prototype target — the
/// synthetic `class_prototype_ref` value or the live decl-prototype heap object
/// — folds onto the stable class-constructor key (`INT32_TAG | class_id`).
///
/// This is the runtime "compatibility shim" alternative to re-targeting the
/// emit (REFLECT-METADATA-SCOPING.md, Task B). It is also GC-safe: the
/// decl-prototype object is a *movable* heap object (`class_decl_prototype_value`
/// allocates and roots it; the GC slot is rewritten on evacuation), so keying on
/// its raw bits would go stale — folding onto the class id sidesteps that with
/// no dedicated GC scanner. Non-prototype targets (constructors, method `.value`
/// closures, plain objects) pass through unchanged.
fn normalize_target_bits(target: f64) -> u64 {
    // Synthetic class-prototype ref → fold onto the class constructor key.
    if let Some(cid) = crate::object::class_prototype_ref_id(target) {
        return crate::object::class_constructor_ref_value(cid).to_bits();
    }
    // Live decl-prototype heap object → fold onto the class constructor key.
    let bits = target.to_bits();
    if (bits >> 48) == (POINTER_TAG >> 48) {
        let ptr = (bits & POINTER_MASK) as usize;
        if let Some(cid) = crate::object::class_id_for_decl_prototype_object(ptr) {
            return crate::object::class_constructor_ref_value(cid).to_bits();
        }
    }
    bits
}

fn make_metadata_key(key: f64, target: f64, property_key: f64) -> Option<MetadataKey> {
    Some(MetadataKey {
        target_bits: normalize_target_bits(target),
        key: metadata_key_part(key)?,
        property_key: metadata_property_key_part(property_key)?,
    })
}

/// Resolve the `propertyKey` argument of a `Reflect.*Metadata(…)` call.
///
/// Returns:
/// - `Some(None)` when the argument is `undefined` — class-level metadata.
/// - `Some(Some(s))` for any value that coerces to a string.
/// - `None` for values we explicitly refuse to key on (e.g. Symbols). The
///   caller treats this as "skip the operation" so we never silently store
///   metadata under an unstable bit-pattern key (#754 review).
fn metadata_property_key_part(property_key: f64) -> Option<Option<String>> {
    if property_key.to_bits() == TAG_UNDEFINED {
        return Some(None);
    }
    metadata_key_part(property_key).map(Some)
}

/// Coerce a metadata key to a stable owned String, or return None if the
/// value cannot be represented as a string key. Returning None makes the
/// caller treat the op as a no-op rather than fabricating a fake key.
///
/// Symbol-keyed metadata is explicitly unsupported (see
/// docs/src/language/decorators.md) — Symbols flow through here and return
/// None rather than colliding on `toString()`'s `"Symbol()"` rendering.
fn metadata_key_part(value: f64) -> Option<String> {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    if let Some((ptr, len)) = crate::string::str_bytes_from_jsvalue(value, &mut scratch) {
        if ptr.is_null() {
            return None;
        }
        if len == 0 {
            return Some(String::new());
        }
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
        return Some(String::from_utf8_lossy(bytes).into_owned());
    }
    if crate::value::is_js_handle(value) {
        let str_ptr = crate::value::js_jsvalue_to_string(value);
        if !str_ptr.is_null() {
            let nb =
                f64::from_bits(crate::value::STRING_TAG | (str_ptr as u64 & 0x0000_FFFF_FFFF_FFFF));
            if let Some((ptr, len)) = crate::string::str_bytes_from_jsvalue(nb, &mut scratch) {
                if !ptr.is_null() {
                    if len == 0 {
                        return Some(String::new());
                    }
                    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
                    return Some(String::from_utf8_lossy(bytes).into_owned());
                }
            }
        }
    }
    // Numbers, booleans, null — coerce through the standard JS path so
    // e.g. `0`, `true`, etc. produce deterministic string keys.
    let coerced = crate::builtins::js_string_coerce(value);
    if !coerced.is_null() {
        let name_ptr =
            unsafe { (coerced as *const u8).add(std::mem::size_of::<crate::StringHeader>()) };
        let name_len = unsafe { (*coerced).byte_len as usize };
        if let Ok(s) =
            std::str::from_utf8(unsafe { std::slice::from_raw_parts(name_ptr, name_len) })
        {
            return Some(s.to_string());
        }
    }
    None
}

fn get_metadata_in_prototype_chain(key: &str, target: f64, property_key: Option<&String>) -> f64 {
    let mut current = target;
    loop {
        let current_bits = normalize_target_bits(current);
        let found = REFLECT_METADATA.with(|store| {
            store
                .borrow()
                .get(&MetadataKey {
                    target_bits: current_bits,
                    key: key.to_string(),
                    property_key: property_key.cloned(),
                })
                .copied()
        });
        if let Some(value) = found {
            return value;
        }

        let next = crate::object::js_object_get_prototype_of(current);
        let next_bits = next.to_bits();
        if next_bits == TAG_NULL || next_bits == TAG_UNDEFINED || next_bits == current_bits {
            return f64::from_bits(TAG_UNDEFINED);
        }
        current = next;
    }
}

fn metadata_keys_for(target: f64, property_key: f64, include_prototypes: bool) -> f64 {
    let Some(wanted_property_key) = metadata_property_key_part(property_key) else {
        let empty = crate::array::js_array_alloc(0);
        return f64::from_bits(POINTER_TAG | ((empty as u64) & POINTER_MASK));
    };

    let keys = REFLECT_METADATA.with(|store| {
        let mut seen = HashSet::new();
        let mut keys = Vec::new();
        let store = store.borrow();
        let mut current = target;

        loop {
            let current_bits = normalize_target_bits(current);
            for metadata_key in store.keys() {
                if metadata_key.target_bits == current_bits
                    && metadata_key.property_key == wanted_property_key
                    && seen.insert(metadata_key.key.clone())
                {
                    keys.push(metadata_key.key.clone());
                }
            }

            if !include_prototypes {
                break;
            }

            let next = crate::object::js_object_get_prototype_of(current);
            let next_bits = next.to_bits();
            if next_bits == TAG_NULL || next_bits == TAG_UNDEFINED || next_bits == current_bits {
                break;
            }
            current = next;
        }

        keys
    });

    let mut values = Vec::with_capacity(keys.len());
    for key in keys {
        values.push(crate::string::js_string_new_sso(
            key.as_ptr(),
            key.len() as u32,
        ));
    }

    let arr = crate::array::js_array_from_f64(values.as_ptr(), values.len() as u32);
    f64::from_bits(POINTER_TAG | ((arr as u64) & POINTER_MASK))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn register_test_class(cid: u32) {
        let mut guard = crate::object::REGISTERED_CLASS_IDS.write().unwrap();
        guard
            .get_or_insert_with(std::collections::HashSet::new)
            .insert(cid);
    }

    // A class's synthetic prototype-ref and its live decl-prototype heap object
    // both fold onto the class-constructor key, so `getMetadata(design:type,
    // SomeClass.prototype, prop)` (class-transformer) finds metadata stored
    // against `ClassRef(SomeClass)` (Perry's emit).
    #[test]
    fn synthetic_prototype_ref_folds_onto_constructor_key() {
        let cid = 0x4242;
        register_test_class(cid);
        let ctor_key = crate::object::class_constructor_ref_value(cid).to_bits();

        let proto_ref = crate::object::class_prototype_ref_value(cid);
        assert_eq!(normalize_target_bits(proto_ref), ctor_key);

        // The constructor ref already IS the key — must pass through unchanged.
        let ctor_ref = crate::object::class_constructor_ref_value(cid);
        assert_eq!(normalize_target_bits(ctor_ref), ctor_key);
    }

    #[test]
    fn decl_prototype_heap_object_folds_onto_constructor_key() {
        let cid = 0x5151;
        register_test_class(cid);
        let fake_proto_ptr: usize = 0x1_0000; // arbitrary; only used as a map key
        {
            let mut guard = crate::object::CLASS_DECL_PROTOTYPE_OBJECTS.write().unwrap();
            guard
                .get_or_insert_with(std::collections::HashMap::new)
                .insert(cid, fake_proto_ptr);
        }
        let target = f64::from_bits(POINTER_TAG | (fake_proto_ptr as u64 & POINTER_MASK));
        assert_eq!(
            normalize_target_bits(target),
            crate::object::class_constructor_ref_value(cid).to_bits()
        );
    }

    #[test]
    fn unrelated_heap_pointer_passes_through_unchanged() {
        // A heap pointer that is not a registered decl-prototype object is left
        // alone (e.g. metadata keyed directly on a plain object/instance).
        let target = f64::from_bits(POINTER_TAG | 0x0AB_CDEF);
        assert_eq!(normalize_target_bits(target), target.to_bits());
    }
}
