//! WTF-8 static property reads for legacy class references.
use super::*;

/// `C.key` for a class constructor value (its function object, or the legacy
/// INT32 immediate / `C.prototype` reference): the class-static lookup. Split
/// out so a class function object is routed here BEFORE the closure arms of
/// the generic read (`get_field_by_name_after_site_miss`), which it
/// would otherwise walk end to end first.
pub(super) unsafe fn class_value_get_wtf8_field(
    class_value: f64,
    class_id: u32,
    is_prototype_ref: bool,
    key: *const crate::StringHeader,
    name: &[u8],
) -> JSValue {
    let scope = crate::gc::RuntimeHandleScope::new();
    let class_value = scope.root_nanbox_f64(class_value);
    let key = scope.root_string_ptr(key);
    if is_prototype_ref {
        if crate::object::native_module::class_has_own_method_bytes(class_id, name) {
            if let Some(value) =
                crate::object::class_registry::class_method_slot_value_bytes(class_id, name)
            {
                return JSValue::from_bits(value);
            }
        }
        if let Some((value, _)) =
            crate::object::class_registry::class_chain_getter_value_bytes(class_id, name, || {
                class_value.get_nanbox_f64()
            })
        {
            return value;
        }
        return key.with_const_ptr::<crate::StringHeader, _>(|key| {
            crate::object::class_value::class_prototype_get(
                class_id,
                key,
                class_value.get_nanbox_f64(),
            )
        });
    }

    // A capture-carrying class declaration may be referenced through its
    // template ClassRef from an earlier-created helper closure. Runtime-added
    // static properties live on the current evaluation's heap class object,
    // so give that object first chance to answer the exact WTF-8 key too.
    if let Some(class_object) = crate::object::class_registry::class_object_value_for_cid(class_id)
    {
        let class_object = JSValue::from_bits(class_object.to_bits());
        if class_object.is_pointer() {
            let class_object = class_object.as_pointer::<ObjectHeader>();
            if !class_object.is_null() {
                let value = key.with_const_ptr::<crate::StringHeader, _>(|key| {
                    js_object_get_field_by_name(class_object, key)
                });
                if !value.is_undefined() {
                    return value;
                }
            }
        }
    }

    if crate::object::class_registry::class_static_key_deleted_bytes(class_id, name) {
        return key.with_const_ptr::<crate::StringHeader, _>(|key| {
            crate::object::class_value::class_prototype_get(
                class_id,
                key,
                class_value.get_nanbox_f64(),
            )
        });
    }
    if let Some(value) = crate::object::class_value::class_static_get_bytes(class_id, name) {
        return JSValue::from_bits(value.to_bits());
    }
    if let Some(value) =
        crate::object::class_value::inherited_static_method_value_bytes(class_id, name)
    {
        return JSValue::from_bits(value.to_bits());
    }
    if let Some(value) = crate::object::class_registry::class_static_accessor_getter_value_bytes(
        class_id,
        name,
        class_value.get_nanbox_f64(),
    ) {
        return JSValue::from_bits(value.to_bits());
    }
    key.with_const_ptr::<crate::StringHeader, _>(|key| {
        crate::object::class_value::class_prototype_get(class_id, key, class_value.get_nanbox_f64())
    })
}
