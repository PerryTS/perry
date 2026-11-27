//! The side-effect import adds callable properties to this realm's Reflect,
//! backed by the same records as compiler-emitted decorator metadata.
use crate::object::{self, property_ops::metadata};

#[no_mangle]
pub extern "C" fn js_reflect_install_metadata(reflect: f64) -> f64 {
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let _no_move = crate::gc::GcSuppressScope::new();
    let Some(receiver) = (unsafe { object::native_call_method::object_ptr_from_value(reflect) })
    else {
        return undefined;
    };
    const INSTALLED: &str = "ReflectMetadataInstalled";
    if object::intrinsic_private_get(reflect, INSTALLED).is_some() {
        return undefined;
    }
    object::intrinsic_private_add(reflect, INSTALLED, 1.0);

    macro_rules! method {
        ($name:literal, $implementation:ident, $arity:tt, $($arg:ident),+) => {{
            extern "C" fn thunk(
                _closure: *const crate::closure::ClosureHeader,
                _this: crate::closure::JsThis,
                $($arg: f64),+
            ) -> f64 {
                metadata::$implementation($($arg),+)
            }
            let keys = unsafe { object::object_keys(receiver) };
            if unsafe { object::keys_find_property_slot_by_bytes(keys, keys.count(), $name.as_bytes()) }.is_none() {
                super::install_static::install_proto_method(
                    receiver, $name, crate::fn_info!(thunk, $arity; with_declared($arity)), $arity,
                );
            }
        }};
    }
    method!(
        "defineMetadata",
        js_reflect_define_metadata,
        4,
        key,
        value,
        target,
        property
    );
    method!(
        "getMetadata",
        js_reflect_get_metadata,
        3,
        key,
        target,
        property
    );
    method!(
        "getOwnMetadata",
        js_reflect_get_own_metadata,
        3,
        key,
        target,
        property
    );
    method!(
        "hasMetadata",
        js_reflect_has_metadata,
        3,
        key,
        target,
        property
    );
    method!(
        "hasOwnMetadata",
        js_reflect_has_own_metadata,
        3,
        key,
        target,
        property
    );
    method!(
        "getMetadataKeys",
        js_reflect_get_metadata_keys,
        2,
        target,
        property
    );
    method!(
        "getOwnMetadataKeys",
        js_reflect_get_own_metadata_keys,
        2,
        target,
        property
    );
    method!(
        "deleteMetadata",
        js_reflect_delete_metadata,
        3,
        key,
        target,
        property
    );
    undefined
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_imports_preserve_deleted_methods_and_other_realms() {
        let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
        let _no_move = crate::gc::GcSuppressScope::new();
        let create = super::super::json_reflect_namespaces::create_reflect_namespace;
        let first = create();
        let second = create();
        let key = crate::string::intern_ascii_literal(b"defineMetadata");
        let read = |obj| object::js_object_get_field_by_name(obj, key).bits();
        let import = |obj| js_reflect_install_metadata(crate::value::js_nanbox_pointer(obj as i64));

        import(first);
        assert_ne!(read(first), crate::value::TAG_UNDEFINED);
        assert_eq!(read(second), crate::value::TAG_UNDEFINED);
        assert_eq!(object::js_object_delete_field(first, key), 1);
        import(first);
        assert_eq!(read(first), crate::value::TAG_UNDEFINED);
    }
}
