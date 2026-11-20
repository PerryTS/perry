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

per_test_global! {
    static REAL_GETS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
}

extern "C" fn counted_receiver_getter(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    REAL_GETS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    this.as_f64()
}

extern "C-unwind" fn throwing_receiver_getter(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    crate::exception::test_throw(this.as_f64())
}

fn define_real_accessor(target: f64, getter: f64) {
    let scope = RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let getter = scope.root_nanbox_f64(getter);
    let descriptor = scope.root_raw_mut_ptr(super::super::js_object_alloc(0, 0));
    for (name, value) in [
        ("get", getter.get_nanbox_f64()),
        (
            "configurable",
            f64::from_bits(crate::JSValue::bool(true).bits()),
        ),
    ] {
        let value = scope.root_nanbox_f64(value);
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        super::super::js_object_set_field_by_name(
            descriptor.get_raw_mut_ptr(),
            key,
            value.get_nanbox_f64(),
        );
    }
    let key = crate::string::js_string_from_bytes(b"promises".as_ptr(), 8);
    super::super::js_object_define_property(
        target.get_nanbox_f64(),
        crate::value::js_nanbox_string(key as i64),
        crate::value::js_nanbox_pointer(descriptor.get_raw_mut_ptr::<ObjectHeader>() as i64),
    );
}

fn read_real_property(namespace: &crate::gc::RuntimeHandle<'_>) -> f64 {
    let key = crate::string::js_string_from_bytes(b"promises".as_ptr(), 8);
    super::super::js_object_get_field_value(namespace.get_nanbox_u64() as i64, key, 0)
}

#[test]
fn real_own_accessor_is_read_per_access_but_reflected_without_invocation() {
    std::thread::spawn(|| {
        let scope = RuntimeHandleScope::new();
        let namespace = scope.root_nanbox_f64(js_create_native_module_namespace(b"fs".as_ptr(), 2));
        let getter = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
            crate::fn_info!(counted_receiver_getter, 0),
            0,
        ));
        define_real_accessor(
            namespace.get_nanbox_f64(),
            crate::value::js_nanbox_pointer(
                getter.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
            ),
        );
        REAL_GETS.store(0, std::sync::atomic::Ordering::Relaxed);
        let key = crate::string::js_string_from_bytes(b"promises".as_ptr(), 8);
        let descriptor =
            scope.root_nanbox_f64(super::super::js_object_get_own_property_descriptor(
                namespace.get_nanbox_f64(),
                crate::value::js_nanbox_string(key as i64),
            ));
        assert_eq!(REAL_GETS.load(std::sync::atomic::Ordering::Relaxed), 0);
        let get_key = crate::string::js_string_from_bytes(b"get".as_ptr(), 3);
        let actual = super::super::js_object_get_field_by_name_f64(
            (descriptor.get_nanbox_u64() & crate::value::POINTER_MASK) as *const ObjectHeader,
            get_key,
        );
        assert_eq!(
            actual.to_bits(),
            crate::value::js_nanbox_pointer(
                getter.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64
            )
            .to_bits()
        );
        assert_eq!(
            read_real_property(&namespace).to_bits(),
            namespace.get_nanbox_u64()
        );
        assert_eq!(read_promises().to_bits(), namespace.get_nanbox_u64());
        assert_eq!(REAL_GETS.load(std::sync::atomic::Ordering::Relaxed), 2);
        define_real_accessor(
            namespace.get_nanbox_f64(),
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        assert_eq!(
            read_real_property(&namespace).to_bits(),
            crate::value::TAG_UNDEFINED
        );
        assert_eq!(read_promises().to_bits(), crate::value::TAG_UNDEFINED);
    })
    .join()
    .unwrap();
}

#[test]
fn real_own_accessor_throw_keeps_exception_identity_on_both_read_paths() {
    std::thread::spawn(|| {
        let scope = RuntimeHandleScope::new();
        let namespace = scope.root_nanbox_f64(js_create_native_module_namespace(b"fs".as_ptr(), 2));
        let getter = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
            crate::fn_info!(unwind throwing_receiver_getter, 0),
            0,
        ));
        define_real_accessor(
            namespace.get_nanbox_f64(),
            crate::value::js_nanbox_pointer(
                getter.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
            ),
        );
        for named in [false, true] {
            let result = crate::exception::js_call_catching(|| {
                if named {
                    read_promises()
                } else {
                    read_real_property(&namespace)
                }
            });
            assert_eq!(result.unwrap_err().to_bits(), namespace.get_nanbox_u64());
        }
    })
    .join()
    .unwrap();
}
