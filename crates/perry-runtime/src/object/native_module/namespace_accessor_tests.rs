use super::*;
use crate::gc::RuntimeHandleScope;

fn read_promises() -> f64 {
    unsafe { js_native_module_property_by_name(b"fs".as_ptr(), 2, b"promises".as_ptr(), 8) }
}

extern "C" fn receiver_getter(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    this.as_f64()
}

#[test]
fn native_namespace_reads_accessor_with_original_receiver() {
    std::thread::spawn(|| {
        let scope = RuntimeHandleScope::new();
        let namespace = scope.root_nanbox_f64(js_create_native_module_namespace(b"fs".as_ptr(), 2));
        let getter = crate::closure::js_closure_alloc(crate::fn_info!(receiver_getter, 0), 0);
        let getter = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(getter as i64));
        super::super::descriptor_state::set_accessor_descriptor(
            (namespace.get_nanbox_u64() & crate::value::POINTER_MASK) as usize,
            "promises".to_string(),
            super::super::descriptor_state::AccessorDescriptor {
                get: getter.get_nanbox_u64(),
                set: 0,
            },
        );
        let actual = read_promises();
        assert_eq!(actual.to_bits(), namespace.get_nanbox_u64());

        super::super::descriptor_state::set_accessor_descriptor(
            (namespace.get_nanbox_u64() & crate::value::POINTER_MASK) as usize,
            "promises".to_string(),
            super::super::descriptor_state::AccessorDescriptor { get: 0, set: 0 },
        );
        assert_eq!(read_promises().to_bits(), crate::value::TAG_UNDEFINED);

        super::super::descriptor_state::clear_accessor_descriptor(
            (namespace.get_nanbox_u64() & crate::value::POINTER_MASK) as usize,
            "promises",
        );
        let key = crate::string::js_string_from_bytes(b"promises".as_ptr(), 8);
        super::super::js_object_set_field_by_name(
            (namespace.get_nanbox_u64() & crate::value::POINTER_MASK) as *mut ObjectHeader,
            key,
            42.0,
        );
        assert_eq!(
            read_promises(),
            42.0,
            "ordinary own data reads remain intact"
        );
    })
    .join()
    .unwrap();
}

#[test]
fn restored_fs_getter_resolves_canonical_value_without_recursion() {
    std::thread::spawn(|| {
        crate::node_submodules::js_node_submod_install_fs_promises();
        let scope = RuntimeHandleScope::new();
        let namespace = scope.root_nanbox_f64(js_create_native_module_namespace(b"fs".as_ptr(), 2));
        let original = scope.root_nanbox_f64(read_promises());
        let getter = scope.root_nanbox_f64(fs_namespace_descriptor_getter_value("promises"));
        super::super::descriptor_state::set_accessor_descriptor(
            (namespace.get_nanbox_u64() & crate::value::POINTER_MASK) as usize,
            "promises".to_string(),
            super::super::descriptor_state::AccessorDescriptor {
                get: getter.get_nanbox_u64(),
                set: 0,
            },
        );
        let actual = read_promises();
        assert_eq!(actual.to_bits(), original.get_nanbox_u64());
    })
    .join()
    .unwrap();
}
