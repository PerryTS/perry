//! Callback-free birth of an ordinary builtin namespace. Declarations are
//! construction input only: the ordinary canonical keys and shape own every
//! property afterward. Nothing is registered before the feature hook runs.
use crate::object::{self, canonical_keys, key_attrs, ObjectHeader};
use crate::JSValue;

enum Value {
    Method(*const crate::closure::JsFunctionInfo, u32),
    Constant(f64),
    Tag,
}

pub(super) struct Member {
    name: &'static str,
    value: Value,
    attrs: u8,
}

impl Member {
    pub(super) const fn method(
        name: &'static str,
        info: *const crate::closure::JsFunctionInfo,
        length: u32,
    ) -> Self {
        Self {
            name,
            value: Value::Method(info, length),
            attrs: key_attrs::ENTRY_NON_ENUMERABLE,
        }
    }

    pub(super) const fn constant(name: &'static str, value: f64) -> Self {
        Self {
            name,
            value: Value::Constant(value),
            attrs: key_attrs::ENTRY_NON_ENUMERABLE
                | key_attrs::ENTRY_NON_WRITABLE
                | key_attrs::ENTRY_NON_CONFIGURABLE,
        }
    }

    pub(super) const fn tag(name: &'static str) -> Self {
        Self {
            name,
            value: Value::Tag,
            attrs: key_attrs::ENTRY_NON_ENUMERABLE | key_attrs::ENTRY_NON_WRITABLE,
        }
    }
}

/// Build distinct ASCII data members in declaration order. The caller's table
/// may contain one final toStringTag member. No accessors or user callbacks
/// run here, and no partial object escapes. Function allocation deliberately
/// retains the individual installer's metadata and non-constructor behavior.
#[cold]
#[inline(never)]
pub(super) fn create(members: &[Member]) -> *mut ObjectHeader {
    // This is the same bounded bootstrap window as individual builtin
    // installation. It also makes the factory safe to exercise independently.
    let _no_move = crate::gc::GcSuppressScope::new();
    let count = u32::try_from(members.len()).expect("builtin namespace size");
    let obj = object::alloc_basic::object_alloc_unpublished(0, count);
    unsafe {
        let header = crate::gc::header_from_trusted_user_ptr(obj.cast()).cast_mut();
        (*header)._reserved |= crate::gc::OBJ_FLAG_HAS_DESCRIPTORS;
        let proof = canonical_keys::SharedLayout::of_receiver(obj)
            .expect("unpublished namespace has shared keys");
        let list = key_attrs::alloc_canonical_key_list(count, false, true);
        let keys = crate::array::array_elements_ptr(list);
        let attrs = key_attrs::keys_attrs(list);
        for (i, member) in members.iter().enumerate() {
            let key = match member.value {
                Value::Tag => {
                    let symbol = crate::symbol::well_known_symbol("toStringTag");
                    assert!(!symbol.is_null(), "bootstrap has well-known symbols");
                    JSValue::pointer(symbol.cast())
                }
                _ => {
                    debug_assert!(member.name.is_ascii());
                    JSValue::string_ptr(
                        crate::string::intern_ascii_literal(member.name.as_bytes()).cast_mut(),
                    )
                }
            };
            // GC_STORE_AUDIT(INIT): unpublished stationary backing under
            // suppression. Publication below shades its external child edges.
            *keys.add(i) = key.bits();
            key_attrs::attrs_write(attrs, i as u32, member.attrs, key);
        }
        (*list).length = count;
        let canonical = canonical_keys::canonicalize_owned(&proof, list, count);
        object::set_object_keys_with_live(obj, canonical.view(), count);
        // Match ordinary bulk birth's incremental-mark handling: the keys
        // are an external shape edge and can contain freshly allocated atoms.
        crate::gc::runtime_shade_external_edge(JSValue::pointer(canonical.as_ptr().cast()).bits());
        if !crate::gc::incremental_mark_barrier_globally_idle() {
            let keys = crate::array::array_elements_ptr(canonical.as_ptr());
            for i in 0..count as usize {
                crate::gc::runtime_shade_external_edge(*keys.add(i));
            }
            let attrs = key_attrs::keys_attrs(canonical.as_ptr());
            if !attrs.is_null() {
                crate::gc::runtime_shade_external_edge(JSValue::pointer(attrs.cast()).bits());
            }
        }
        for (i, member) in members.iter().enumerate() {
            let value = match member.value {
                Value::Method(info, length) => {
                    let closure =
                        super::install_static::create_builtin_method(member.name, info, length);
                    if closure.is_null() {
                        crate::value::TAG_UNDEFINED
                    } else {
                        crate::value::js_nanbox_pointer(closure as i64).to_bits()
                    }
                }
                Value::Constant(value) => JSValue::number(value).bits(),
                Value::Tag => JSValue::string_ptr(crate::string::js_string_from_bytes(
                    member.name.as_ptr(),
                    member.name.len() as u32,
                ))
                .bits(),
            };
            object::store_object_field_slot(obj, i, value);
        }
    }
    obj
}

#[cfg(test)]
mod tests;
