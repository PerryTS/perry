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
            let parent = crate::object::bound_native_callable_export_value(&crate::object::native_module::namespaces::v8::NM_V8, "v8", parent_name);
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
mod attach_reach_tests {
    use super::*;

    /// `attach` must install on the prototype that the constructor it is HANDED
    /// exposes. If it does, then a receiver resolved from that same constructor
    /// sees the methods, and the remaining question is only which object the
    /// module namespace hands a TS program.
    #[test]
    fn attach_installs_on_the_prototype_of_the_constructor_it_is_given() {
        let _no_gc = crate::gc::GcSuppressScope::new();
        unsafe {
            let ctor = crate::object::bound_native_callable_export_value(
                &crate::object::native_module::namespaces::v8::NM_V8,
                "v8",
                "Serializer",
            );
            let proto = crate::object::js_function_prototype_value_for_read(ctor);
            let before = crate::object::method_call::spec_get_value(proto, b"writeHeader");
            assert_eq!(
                before.to_bits(),
                crate::value::TAG_UNDEFINED,
                "fixture: the method must not exist yet"
            );
            let returned = attach("Serializer", ctor);
            assert_eq!(
                returned.to_bits(),
                ctor.to_bits(),
                "attach returns its constructor"
            );
            let after = crate::object::method_call::spec_get_value(proto, b"writeHeader");
            assert_ne!(
                after.to_bits(),
                crate::value::TAG_UNDEFINED,
                "attach must install writeHeader on this constructor's prototype"
            );
            // The object a program sees as `v8.Serializer` is the one the
            // module namespace resolves, which is NOT necessarily the value
            // this helper mints.
            let module = crate::value::js_nanbox_string(crate::string::js_string_from_bytes(
                b"v8".as_ptr(),
                2,
            ) as i64);
            let property = crate::value::js_nanbox_string(crate::string::js_string_from_bytes(
                b"Serializer".as_ptr(),
                10,
            ) as i64);
            let from_namespace = crate::object::js_native_module_esm_export_value_vt(
                &crate::object::native_module::namespaces::v8::NM_V8,
                module,
                property,
            );
            assert_eq!(
                from_namespace.to_bits(),
                ctor.to_bits(),
                "the namespace value and bound_native_callable_export_value must be the same object,                  or attach writes a prototype no program reads"
            );
        }
    }
}
