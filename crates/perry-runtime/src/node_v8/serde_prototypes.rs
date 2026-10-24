//! Real prototype methods for the implemented v8 serializer operations.
use super::*;
use crate::closure::{ClosureHeader, JsThis};

fn receiver(this: JsThis, serializer: bool) -> f64 {
    let value = this.as_f64();
    let valid = crate::value::addr_class::pointer_value_header(value.to_bits())
        .is_some_and(|header| header.obj_type == crate::gc::GC_TYPE_OBJECT);
    if valid {
        let obj = (value.to_bits() & crate::value::POINTER_MASK) as *const ObjectHeader;
        // SAFETY: the canonical tagged-value admission proved an object header.
        let module = unsafe {
            ((*obj).class_id == crate::object::NATIVE_MODULE_CLASS_ID)
                .then(|| crate::object::native_module::read_native_module_name(obj))
                .flatten()
        };
        if matches!(
            (serializer, module.as_deref()),
            (true, Some("v8.Serializer" | "v8.DefaultSerializer"))
                | (false, Some("v8.Deserializer" | "v8.DefaultDeserializer"))
        ) {
            return value;
        }
    }
    crate::node_submodules::diagnostics::throw_type_error_no_code(b"Illegal invocation")
}

macro_rules! method {
    ($name:ident, $helper:ident, $serializer:expr $(, $arg:ident)*) => {
        unsafe extern "C-unwind" fn $name(_: *const ClosureHeader, this: JsThis $(, $arg: f64)*) -> f64 {
            super::$helper(receiver(this, $serializer) $(, $arg)*)
        }
    };
}
method!(write_header, v8_serializer_write_header, true);
method!(write_value, v8_serializer_write_value, true, value);
method!(release_buffer, v8_serializer_release_buffer, true);
method!(write_uint32, v8_serializer_write_uint32, true, value);
method!(write_uint64, v8_serializer_write_uint64, true, hi, lo);
method!(write_double, v8_serializer_write_double, true, value);
method!(write_raw_bytes, v8_serializer_write_raw_bytes, true, value);
method!(read_header, v8_deserializer_read_header, false);
method!(read_value, v8_deserializer_read_value, false);
method!(read_uint32, v8_deserializer_read_uint32, false);
method!(read_uint64, v8_deserializer_read_uint64, false);
method!(read_double, v8_deserializer_read_double, false);
method!(
    read_raw_bytes,
    v8_deserializer_read_raw_bytes,
    false,
    length
);

pub(super) unsafe fn attach(name: &str, constructor: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let constructor = scope.root_nanbox_f64(constructor);
    let methods = match name {
        "Serializer" => vec![
            (
                "writeHeader",
                crate::fn_info!(unwind write_header, 0; with_flags(crate::closure::FN_BUILTIN | crate::closure::FN_NON_CONSTRUCTOR)),
                0,
            ),
            (
                "writeValue",
                crate::fn_info!(unwind write_value, 1; with_flags(crate::closure::FN_BUILTIN | crate::closure::FN_NON_CONSTRUCTOR)),
                0,
            ),
            (
                "releaseBuffer",
                crate::fn_info!(unwind release_buffer, 0; with_flags(crate::closure::FN_BUILTIN | crate::closure::FN_NON_CONSTRUCTOR)),
                0,
            ),
            (
                "writeUint32",
                crate::fn_info!(unwind write_uint32, 1; with_flags(crate::closure::FN_BUILTIN | crate::closure::FN_NON_CONSTRUCTOR)),
                0,
            ),
            (
                "writeUint64",
                crate::fn_info!(unwind write_uint64, 2; with_flags(crate::closure::FN_BUILTIN | crate::closure::FN_NON_CONSTRUCTOR)),
                0,
            ),
            (
                "writeDouble",
                crate::fn_info!(unwind write_double, 1; with_flags(crate::closure::FN_BUILTIN | crate::closure::FN_NON_CONSTRUCTOR)),
                0,
            ),
            (
                "writeRawBytes",
                crate::fn_info!(unwind write_raw_bytes, 1; with_flags(crate::closure::FN_BUILTIN | crate::closure::FN_NON_CONSTRUCTOR)),
                0,
            ),
        ],
        "Deserializer" => vec![
            (
                "readHeader",
                crate::fn_info!(unwind read_header, 0; with_flags(crate::closure::FN_BUILTIN | crate::closure::FN_NON_CONSTRUCTOR)),
                0,
            ),
            (
                "readValue",
                crate::fn_info!(unwind read_value, 0; with_flags(crate::closure::FN_BUILTIN | crate::closure::FN_NON_CONSTRUCTOR)),
                0,
            ),
            (
                "readUint32",
                crate::fn_info!(unwind read_uint32, 0; with_flags(crate::closure::FN_BUILTIN | crate::closure::FN_NON_CONSTRUCTOR)),
                0,
            ),
            (
                "readUint64",
                crate::fn_info!(unwind read_uint64, 0; with_flags(crate::closure::FN_BUILTIN | crate::closure::FN_NON_CONSTRUCTOR)),
                0,
            ),
            (
                "readDouble",
                crate::fn_info!(unwind read_double, 0; with_flags(crate::closure::FN_BUILTIN | crate::closure::FN_NON_CONSTRUCTOR)),
                0,
            ),
            (
                "readRawBytes",
                crate::fn_info!(unwind read_raw_bytes, 1; with_flags(crate::closure::FN_BUILTIN | crate::closure::FN_NON_CONSTRUCTOR)),
                1,
            ),
        ],
        "DefaultSerializer" | "DefaultDeserializer" => {
            let parent_name = name.strip_prefix("Default").unwrap();
            let parent = crate::object::bound_native_callable_export_value("v8", parent_name);
            return crate::object::native_module::constructor_shapes::link_parent(
                constructor.get_nanbox_f64(),
                parent,
            );
        }
        _ => return constructor.get_nanbox_f64(),
    };
    let prototype =
        crate::object::js_function_prototype_value_for_read(constructor.get_nanbox_f64());
    let prototype = scope.root_nanbox_f64(prototype);
    for (name, info, length) in methods {
        let method = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc_singleton(info));
        method.with_mut_ptr(|method| {
            crate::object::native_module::set_bound_native_closure_name(method, name)
        });
        method.with_const_ptr::<ClosureHeader, _>(|method| {
            crate::object::native_module::set_builtin_closure_length(method as usize, length)
        });
        let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let key = scope.root_string_ptr(key);
        key.with_const_ptr(|key| {
            method.with_const_ptr::<ClosureHeader, _>(|method| {
                crate::object::js_object_set_field_by_name(
                    crate::value::js_nanbox_get_pointer(prototype.get_nanbox_f64())
                        as *mut ObjectHeader,
                    key,
                    crate::value::js_nanbox_pointer(method as i64),
                )
            })
        });
        crate::object::set_builtin_property_attrs(
            crate::value::js_nanbox_get_pointer(prototype.get_nanbox_f64()) as usize,
            name.to_string(),
            crate::object::PropertyAttrs::new(true, false, true),
        );
    }
    constructor.get_nanbox_f64()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializer_instances_share_declared_methods_and_prototypes() {
        crate::object::js_nm_install_v8();
        let roots = crate::gc::RuntimeHandleScope::new();
        let first = roots.root_nanbox_f64(js_v8_serializer_new(undefined()));
        let second = roots.root_nanbox_f64(js_v8_serializer_new(undefined()));
        let constructor = roots.root_nanbox_f64(crate::object::bound_native_callable_export_value(
            "v8",
            "Serializer",
        ));
        let prototype = roots.root_nanbox_f64(crate::object::js_function_prototype_value_for_read(
            constructor.get_nanbox_f64(),
        ));
        assert_eq!(
            crate::object::js_object_get_prototype_of(first.get_nanbox_f64()).to_bits(),
            prototype.get_nanbox_u64()
        );
        for name in [
            "writeHeader",
            "writeValue",
            "releaseBuffer",
            "writeUint32",
            "writeUint64",
            "writeDouble",
            "writeRawBytes",
        ] {
            let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
            let key = roots.root_string_ptr(key);
            let read = |value| {
                key.with_const_ptr(|key| {
                    crate::object::js_object_get_field_by_name(
                        crate::value::js_nanbox_get_pointer(value) as *const ObjectHeader,
                        key,
                    )
                })
            };
            let actual = read(first.get_nanbox_f64());
            assert!(actual.is_pointer(), "{name} must be a real method value");
            let method = roots.root_nanbox_f64(f64::from_bits(actual.bits()));
            let other = read(second.get_nanbox_f64());
            assert_eq!(method.get_nanbox_u64(), other.bits(), "{name}");
            let declared = read(prototype.get_nanbox_f64());
            assert_eq!(method.get_nanbox_u64(), declared.bits(), "{name}");
        }
    }

    #[test]
    fn default_constructor_alias_uses_the_real_inheritance_chain() {
        crate::object::js_nm_install_v8();
        let roots = crate::gc::RuntimeHandleScope::new();
        let value = unsafe {
            super::super::serde_constructor("v8", "DefaultSerializer", std::ptr::null(), 0).unwrap()
        };
        let value = roots.root_nanbox_f64(value);
        let child = roots.root_nanbox_f64(crate::object::bound_native_callable_export_value(
            "v8",
            "DefaultSerializer",
        ));
        let parent = roots.root_nanbox_f64(crate::object::bound_native_callable_export_value(
            "v8",
            "Serializer",
        ));
        assert_eq!(
            crate::object::js_object_get_prototype_of(child.get_nanbox_f64()).to_bits(),
            parent.get_nanbox_u64()
        );
        let child_proto = roots.root_nanbox_f64(
            crate::object::js_function_prototype_value_for_read(child.get_nanbox_f64()),
        );
        let parent_proto = roots.root_nanbox_f64(
            crate::object::js_function_prototype_value_for_read(parent.get_nanbox_f64()),
        );
        assert_eq!(
            crate::object::js_object_get_prototype_of(value.get_nanbox_f64()).to_bits(),
            child_proto.get_nanbox_u64()
        );
        assert_eq!(
            crate::object::js_object_get_prototype_of(child_proto.get_nanbox_f64()).to_bits(),
            parent_proto.get_nanbox_u64()
        );
    }
}
