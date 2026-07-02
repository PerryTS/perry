//! Proxy trap result invariant enforcement (ECMA-262 §10.5).
//!
//! After a `get`/`set`/`has`/`deleteProperty`/`defineProperty` trap runs, the
//! spec re-reads the *target's* own property descriptor and throws a
//! `TypeError` when the trap result is inconsistent with a non-configurable (or
//! non-extensible) target. These checks make a Proxy unable to lie about the
//! invariant parts of its target — they are what the bulk of the
//! `built-ins/Proxy/*/...throws` tests exercise.

use super::{extract_pointer, throw_type_error, TAG_NULL, TAG_UNDEFINED};

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
pub(super) fn enforce_define_property_invariant(target: f64, property_key: f64, descriptor: f64) {
    let extensible = target_is_extensible(target);
    let setting_config_false = desc_has(descriptor, b"configurable")
        && !truthy({
            let ptr = extract_pointer(descriptor.to_bits()) as *const crate::ObjectHeader;
            desc_field(ptr, b"configurable")
        });

    match target_own_prop(target, property_key) {
        None => {
            if !extensible {
                throw_type_error(
                    "proxy defineProperty trap added a property to a non-extensible target",
                );
            }
            if setting_config_false {
                throw_type_error(
                    "proxy defineProperty trap added a non-configurable property absent from the target",
                );
            }
        }
        Some(prop) => {
            if !is_compatible_descriptor(&prop, descriptor) {
                throw_type_error(
                    "proxy defineProperty trap reported an incompatible descriptor for the target",
                );
            }
            if setting_config_false && prop.configurable {
                throw_type_error(
                    "proxy defineProperty trap made a configurable target property non-configurable",
                );
            }
        }
    }
}

/// A conservative IsCompatiblePropertyDescriptor check against a
/// non-configurable existing target property. For a configurable target
/// property any redefinition is compatible.
fn is_compatible_descriptor(current: &TargetProp, descriptor: f64) -> bool {
    if current.configurable {
        return true;
    }
    let ptr = extract_pointer(descriptor.to_bits()) as *const crate::ObjectHeader;
    let desc_is_accessor = desc_has(descriptor, b"get") || desc_has(descriptor, b"set");
    let desc_is_data = desc_has(descriptor, b"value") || desc_has(descriptor, b"writable");

    // ECMA-262 §10.5.6 step 7.a.i: a descriptor that attempts to set
    // `configurable:true` on a non-configurable own property is always invalid,
    // regardless of whether it is generic, data, or accessor.
    if desc_has(descriptor, b"configurable") && truthy(desc_field(ptr, b"configurable")) {
        return false;
    }

    // A generic descriptor — only `configurable`/`enumerable`, with no
    // type-defining field (get/set or value/writable) — is compatible with
    // any non-configurable current property. Per ValidateAndApplyProperty-
    // Descriptor, IsGenericDescriptor short-circuits the data/accessor-type
    // and value/writable checks. `Object.freeze`/`Object.seal` of a Proxy
    // drives exactly such a descriptor (`{configurable:false}`) onto every
    // own key, including an accessor key, so treating it as a data descriptor
    // here wrongly aborted with "incompatible descriptor" (test262
    // freeze/seal proxy-with-defineProperty-handler).
    if !desc_is_accessor && !desc_is_data {
        return true;
    }

    // A non-configurable property cannot switch between data and accessor.
    if desc_is_accessor != current.is_accessor {
        return false;
    }
    if current.is_accessor {
        // Accessor: a specified get/set must SameValue the current one.
        if desc_has(descriptor, b"get") {
            let g = desc_field(ptr, b"get");
            let cur_undef = current.getter_undefined;
            if (g.to_bits() == TAG_UNDEFINED) != cur_undef {
                return false;
            }
        }
        if desc_has(descriptor, b"set") {
            let s = desc_field(ptr, b"set");
            if (s.to_bits() == TAG_UNDEFINED) != current.setter_undefined {
                return false;
            }
        }
        return true;
    }

    // Data: a non-writable property cannot become writable, and its value
    // cannot change (unless made writable, which is itself forbidden above).
    if desc_has(descriptor, b"writable") {
        let w = truthy(desc_field(ptr, b"writable"));
        if w && !current.writable {
            return false;
        }
    }
    if !current.writable && desc_has(descriptor, b"value") {
        let v = desc_field(ptr, b"value");
        if !same_value(v, current.value) {
            return false;
        }
    }
    let _ = TAG_NULL;
    true
}
