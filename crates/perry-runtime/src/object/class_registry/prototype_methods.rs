use super::*;
use std::collections::HashMap;

const CLASS_LEXICAL_BINDING_KEY: &str = "#<perry:private-class-lexical-binding>";

/// Read/write storage for the outer mutable binding introduced by a class
/// declaration.  The class body's same-spelled inner binding is lowered
/// directly to its ClassRef and never reaches these helpers.
#[no_mangle]
pub extern "C" fn js_class_lexical_binding_get(class_ref: f64) -> f64 {
    let Some(class_id) = class_ref_id(class_ref) else {
        return class_ref;
    };
    class_own_static_field_value(class_id, CLASS_LEXICAL_BINDING_KEY).unwrap_or(class_ref)
}

#[no_mangle]
pub extern "C" fn js_class_lexical_binding_set(class_ref: f64, value: f64) -> f64 {
    if let Some(class_id) = class_ref_id(class_ref) {
        class_dynamic_prop_root_store(class_id, CLASS_LEXICAL_BINDING_KEY, value);
    }
    value
}

/// Register a static field value on a class so `Cls.field` (when `Cls` is
/// accessed via dynamic dispatch — e.g. through an Any-typed local) finds
/// the value via the runtime path. Codegen calls this at module init for
/// every static field initializer in addition to writing the value to the
/// per-field module global. `global_slot` binds those two storage views so a
/// later computed/member write can update the cell used by direct reads.
/// Refs #420 / #618 followup / #9526. Static-field values are stored in
/// CLASS_DYNAMIC_PROPS keyed by class_id.
#[no_mangle]
pub unsafe extern "C" fn js_class_register_static_field(
    class_id: u32,
    name_ptr: *const u8,
    name_len: usize,
    value: f64,
    global_slot: *mut f64,
) {
    if class_id == 0 || name_ptr.is_null() || name_len == 0 {
        return;
    }
    let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) else {
        return;
    };
    class_register_declared_static_global_slot(class_id, name, global_slot);
    class_dynamic_prop_root_store(class_id, name, value);
    // DefineField: a static field is an ordinary writable, enumerable,
    // configurable own property — also when it replaces the class's
    // intrinsic `name` / `length`.
    crate::object::class_value::note_static_field_defined(class_id, name);
}

/// Define a static PRIVATE field as an `ENTRY_PRIVATE` slot of the class
/// function's bag. It never has a corresponding public property or alias.
#[no_mangle]
pub unsafe extern "C" fn js_class_register_static_private_field(
    class_id: u32,
    name_ptr: *const u8,
    name_len: usize,
    value: f64,
    _global_slot: *mut f64,
) {
    if class_id == 0 || name_ptr.is_null() || name_len == 0 {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let receiver = scope.root_nanbox_f64(crate::object::class_constructor_ref_value(class_id));
    let key = crate::string::js_string_from_bytes(name_ptr, name_len as u32);
    crate::object::field_get_set::define_static_private_field(
        receiver.get_nanbox_f64(),
        key,
        value.get_nanbox_f64(),
    );
}

/// Define a static PRIVATE field of a fresh class evaluation on its class
/// object, directly in the private namespace of its own shape.
#[no_mangle]
pub unsafe extern "C" fn js_class_object_define_static_private(
    class_object: *mut crate::object::ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) {
    if key.is_null() {
        return;
    }
    crate::object::field_get_set::define_static_private_field(
        crate::value::js_nanbox_pointer(class_object as i64),
        key,
        value,
    );
}

/// Read a computed instance-field key resolved at ClassDefinitionEvaluation.
/// Fresh class values carry the hidden slot on their heap class object; plain
/// class references use the class-id static side table.
#[no_mangle]
pub unsafe extern "C" fn js_class_computed_field_key(
    receiver: f64,
    class_id: u32,
    name_ptr: *const u8,
    name_len: usize,
) -> f64 {
    if name_ptr.is_null() || name_len == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if let Some(owner) = crate::object::private_evaluation_brand_value(receiver) {
        let value = crate::object::js_object_get_own_field_or_undef(owner, name_ptr, name_len);
        if value.to_bits() != crate::value::TAG_UNDEFINED {
            return value;
        }
    }
    let bytes = std::slice::from_raw_parts(name_ptr, name_len);
    let Ok(name) = std::str::from_utf8(bytes) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    class_own_static_field_value(class_id, name)
        .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED))
}

/// Compatibility entry points for HIR assembled by embedders. The prototype
/// is read before the value is stored; its ordinary [[Set]] owns the mutation.
pub(crate) fn class_prototype_set(class_id: u32, name: String, value_bits: u64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_u64(value_bits);
    // The holder an instance read walks (declared, else synthetic); a class
    // with neither builds its declared prototype.
    let proto = super::prototype_objects::class_holder_prototype(class_id);
    let proto = if proto.is_null() {
        class_decl_prototype_value(class_id)
    } else {
        crate::value::js_nanbox_pointer(proto as i64)
    };
    let proto = scope.root_nanbox_f64(proto);
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let key = crate::value::js_nanbox_string(key as i64);
    crate::proxy::js_put_value_set(
        proto.get_nanbox_f64(),
        key,
        value.get_nanbox_f64(),
        proto.get_nanbox_f64(),
        1,
    );
}

#[no_mangle]
pub unsafe extern "C" fn js_register_prototype_method(
    class_id: u32,
    name_ptr: *const u8,
    name_len: usize,
    value: f64,
) {
    if name_ptr.is_null() {
        return;
    }
    if let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
        class_prototype_set(class_id, name.to_string(), value.to_bits());
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_get_function_prototype_method(
    func_value: f64,
    name_ptr: *const u8,
    name_len: usize,
) -> f64 {
    if name_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_nanbox_f64(js_function_prototype_value_for_read(func_value));
    let key = crate::string::js_string_from_bytes(name_ptr, name_len as u32);
    super::super::field_get_set::js_object_get_field_by_name_f64(
        proto.get_nanbox_f64().to_bits() as *const ObjectHeader,
        key,
    )
}

#[no_mangle]
pub unsafe extern "C" fn js_register_function_prototype_method(
    func_value: f64,
    name_ptr: *const u8,
    name_len: usize,
    value: f64,
) -> u32 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let func = scope.root_nanbox_f64(func_value);
    let value = scope.root_nanbox_f64(value);
    let proto = scope.root_nanbox_f64(js_function_prototype_value_for_read(func.get_nanbox_f64()));
    if !name_ptr.is_null() {
        let key = crate::string::js_string_from_bytes(name_ptr, name_len as u32);
        crate::proxy::js_put_value_set(
            proto.get_nanbox_f64(),
            crate::value::js_nanbox_string(key as i64),
            value.get_nanbox_f64(),
            proto.get_nanbox_f64(),
            1,
        );
    }
    function_class_id(func.get_nanbox_f64())
}

/// Get-or-allocate a synthetic class id keyed by a function value's
/// NaN-boxed bits. Used by `js_register_function_prototype_method` (HIR
/// "Func.prototype.x = fn" recogniser) and `js_new_function_construct`
/// (HIR "new Func(args)" allocator) so both sides agree on the same id
/// — the instance's `(*obj).class_id` lands in the same bucket the
/// method registration stored against. Returns 0 if `func_value` isn't a
/// POINTER_TAG'd value (callable shape requirement).
pub(crate) fn synthetic_class_id_for_function(func_value: f64) -> u32 {
    let func_bits = func_value.to_bits();
    // Require a verified closure shape so we don't store arbitrary
    // POINTER_TAG'd pointers (arrays, objects, etc. all share the tag)
    // in `FUNCTION_CLASS_IDS`. The bits-as-key invariant only makes
    // sense for callable values that produced a stable singleton
    // closure pointer.
    if !is_callable_function_value(func_value) {
        return 0;
    }
    let existing = FUNCTION_CLASS_IDS.with(|table| {
        let read = table.read().unwrap();
        if let Some(map) = read.as_ref() {
            if let Some(&existing) = map.get(&func_bits) {
                return Some(existing);
            }
        }
        None
    });
    if let Some(existing) = existing {
        return existing;
    }
    let new_cid = super::prototype_objects::alloc_synthetic_class_id();
    FUNCTION_CLASS_IDS.with(|table| {
        let mut write = table.write().unwrap();
        if write.is_none() {
            *write = Some(HashMap::new());
        }
        write.as_mut().unwrap().insert(func_bits, new_cid);
    });
    unsafe { js_register_class_id(new_cid) };
    new_cid
}
