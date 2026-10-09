//! Proxy trap result invariant enforcement (ECMA-262 §10.5).
//!
//! After a `get`/`set`/`has`/`deleteProperty`/`defineProperty` trap runs, the
//! spec re-reads the *target's* own property descriptor and throws a
//! `TypeError` when the trap result is inconsistent with a non-configurable (or
//! non-extensible) target. These checks make a Proxy unable to lie about the
//! invariant parts of its target — they are what the bulk of the
//! `built-ins/Proxy/*/...throws` tests exercise.

use super::{extract_pointer, throw_type_error, TAG_UNDEFINED};

/// A target's own-property descriptor, reduced to the fields the invariant
/// checks need. Built from `[[GetOwnProperty]]` (FromPropertyDescriptor), so a
/// data descriptor populates `value`/`writable` and an accessor descriptor
/// populates `getter_undefined`/`setter_undefined`.
struct TargetProp {
    configurable: bool,
    is_accessor: bool,
    writable: bool,
    value: f64,
    getter_undefined: bool,
    setter_undefined: bool,
}

fn truthy(v: f64) -> bool {
    crate::value::js_is_truthy(v) != 0
}

fn desc_field(desc_ptr: *const crate::ObjectHeader, name: &[u8]) -> f64 {
    if desc_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_get_field_by_name_f64(desc_ptr, key)
}

fn desc_has(desc: f64, name: &[u8]) -> bool {
    let ptr = extract_pointer(desc.to_bits()) as *mut crate::ObjectHeader;
    if ptr.is_null() {
        return false;
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    unsafe { crate::object::own_key_present(ptr, key) }
}

/// Read `target.[[GetOwnProperty]](property_key)` as a `TargetProp`. Returns
/// `None` when the target has no such own property (descriptor is `undefined`).
fn target_own_prop(target: f64, property_key: f64) -> Option<TargetProp> {
    let desc = crate::object::js_object_get_own_property_descriptor(target, property_key);
    if desc.to_bits() == TAG_UNDEFINED {
        return None;
    }
    let ptr = extract_pointer(desc.to_bits()) as *const crate::ObjectHeader;
    if ptr.is_null() {
        return None;
    }
    let is_accessor = desc_has(desc, b"get") || desc_has(desc, b"set");
    Some(TargetProp {
        configurable: truthy(desc_field(ptr, b"configurable")),
        is_accessor,
        writable: truthy(desc_field(ptr, b"writable")),
        value: desc_field(ptr, b"value"),
        getter_undefined: desc_field(ptr, b"get").to_bits() == TAG_UNDEFINED,
        setter_undefined: desc_field(ptr, b"set").to_bits() == TAG_UNDEFINED,
    })
}

fn same_value(a: f64, b: f64) -> bool {
    crate::value::js_jsvalue_same_value_zero(a, b) != 0
}

fn target_is_extensible(target: f64) -> bool {
    !crate::object::obj_value_no_extend(target)
}

/// `[[Get]]` invariant: a non-configurable, non-writable data property forces
/// the trap result to SameValue the target value; a non-configurable accessor
/// with no getter forces an `undefined` trap result.
pub(super) fn enforce_get_invariant(target: f64, property_key: f64, trap_result: f64) {
    let Some(prop) = target_own_prop(target, property_key) else {
        return;
    };
    if prop.configurable {
        return;
    }
    if !prop.is_accessor {
        if !prop.writable && !same_value(trap_result, prop.value) {
            throw_type_error(
                "proxy get trap returned a different value for a non-writable, non-configurable property",
            );
        }
    } else if prop.getter_undefined && trap_result.to_bits() != TAG_UNDEFINED {
        throw_type_error(
            "proxy get trap returned a value for a non-configurable accessor with an undefined getter",
        );
    }
}

/// `[[Set]]` invariant (checked only when the trap returned a truthy result): a
/// non-configurable, non-writable data property requires the written value to
/// SameValue the target value; a non-configurable accessor requires a setter.
pub(super) fn enforce_set_invariant(target: f64, property_key: f64, value: f64) {
    let Some(prop) = target_own_prop(target, property_key) else {
        return;
    };
    if prop.configurable {
        return;
    }
    if !prop.is_accessor {
        if !prop.writable && !same_value(value, prop.value) {
            throw_type_error(
                "proxy set trap reported success for a non-writable, non-configurable property",
            );
        }
    } else if prop.setter_undefined {
        throw_type_error(
            "proxy set trap reported success for a non-configurable accessor with an undefined setter",
        );
    }
}

/// `[[HasProperty]]` invariant (checked only when the trap returned `false`): a
/// non-configurable own key, or any own key on a non-extensible target, cannot
/// be hidden.
pub(super) fn enforce_has_false_invariant(target: f64, property_key: f64) {
    let Some(prop) = target_own_prop(target, property_key) else {
        return;
    };
    if !prop.configurable {
        throw_type_error("proxy has trap returned false for a non-configurable property");
    }
    if !target_is_extensible(target) {
        throw_type_error("proxy has trap returned false for a property of a non-extensible target");
    }
}

/// `[[Delete]]` invariant (checked only when the trap returned a truthy result):
/// a non-configurable own key, or any own key on a non-extensible target,
/// cannot be reported as deleted.
pub(super) fn enforce_delete_invariant(target: f64, property_key: f64) {
    let Some(prop) = target_own_prop(target, property_key) else {
        return;
    };
    if !prop.configurable {
        throw_type_error(
            "proxy deleteProperty trap reported success for a non-configurable property",
        );
    }
    if !target_is_extensible(target) {
        throw_type_error(
            "proxy deleteProperty trap reported success for a property of a non-extensible target",
        );
    }
}

/// `[[DefineOwnProperty]]` invariant (checked only when the trap returned a
/// truthy result). Implements the key rejections from ValidateAndApplyProperty
/// against the target:
///  * defining a new property on a non-extensible target,
///  * adding a non-configurable property the target doesn't have,
///  * redefining a non-configurable target property in an incompatible way.
pub(super) fn enforce_define_property_invariant(
    target: f64,
    property_key: f64,
    descriptor: &crate::object::object_ops::DescView<'_>,
) {
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let target = scope.root_nanbox_u64(target.to_bits());
        let key = scope.root_nanbox_f64(property_key);
        let current = scope.root_nanbox_f64(crate::object::js_object_get_own_property_descriptor(
            f64::from_bits(target.get_nanbox_u64()),
            key.get_nanbox_f64(),
        ));
        let extensible = crate::value::js_is_truthy(crate::object::js_object_is_extensible(
            f64::from_bits(target.get_nanbox_u64()),
        )) != 0;
        let setting_config_false = descriptor.flag(b"configurable") == Some(false);
        if current.get_nanbox_u64() == TAG_UNDEFINED {
            if !extensible {
                throw_type_error(
                    "proxy defineProperty trap added a property to a non-extensible target",
                );
            }
            if setting_config_false {
                throw_type_error("proxy defineProperty trap added a non-configurable property absent from the target");
            }
            return;
        }
        let current = crate::object::object_ops::decode_own_descriptor_result(&scope, &current);
        if !crate::object::object_ops::descriptor_compatible_with_current(&current, descriptor) {
            throw_type_error(
                "proxy defineProperty trap reported an incompatible descriptor for the target",
            );
        }
        if setting_config_false && current.flag(b"configurable") == Some(true) {
            throw_type_error(
                "proxy defineProperty trap made a configurable target property non-configurable",
            );
        }
        if current.flag(b"configurable") == Some(false)
            && current.flag(b"writable") == Some(true)
            && descriptor.flag(b"writable") == Some(false)
        {
            throw_type_error(
                "proxy defineProperty trap made a writable target property non-writable",
            );
        }
    }
}
