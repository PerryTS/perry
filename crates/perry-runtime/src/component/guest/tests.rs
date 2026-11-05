use super::*;

const CONTRACT: &[u8] = br#"{"schema_version":1,"name":"test:initialization/plugin","imports":[{"name":"","functions":[{"name":"echo","params":[{"name":"value","ty":{"kind":"string"}}],"result":{"kind":"string"},"kind":{"kind":"function"}}],"resources":{}}],"exports":[],"types":{}}"#;

unsafe extern "C" fn echo(
    frame: *mut std::ffi::c_void,
    interface: u32,
    function: u32,
    args: u32,
) -> u32 {
    if interface != 0 || function != 0 {
        frame::perry_component_frame_fail(frame, b"Wrong import index".as_ptr(), 18);
        return 0;
    }
    frame::perry_component_frame_array_get(frame, args, 0)
}

#[test]
fn imports_work_during_initialization_before_exports_are_registered() {
    crate::test_isolation::in_own_process(|| {
        let scope = RuntimeHandleScope::new();
        let global = scope.root_nanbox_f64(crate::object::js_get_global_this());
        let previous = scope.root_nanbox_f64(maybe_registry());
        let ops = ComponentGuestOps {
            abi_version: GUEST_ABI_VERSION,
            size: std::mem::size_of::<ComponentGuestOps>(),
            contract: CONTRACT.as_ptr(),
            contract_len: CONTRACT.len(),
            import: echo,
            resource: unused_resource,
        };
        struct Restore<'a> {
            global: crate::gc::RuntimeHandle<'a>,
            registry: crate::gc::RuntimeHandle<'a>,
            ops: *mut ComponentGuestOps,
        }
        impl Drop for Restore<'_> {
            fn drop(&mut self) {
                GUEST_OPS.store(self.ops, Ordering::Release);
                unsafe {
                    bag::store(
                        object_pointer(self.global.get_nanbox_f64()).unwrap() as usize,
                        COMPONENT_GUEST,
                        self.registry.get_nanbox_f64(),
                    );
                }
            }
        }
        let _restore = Restore {
            global,
            registry: previous,
            ops: GUEST_OPS.swap(std::ptr::null_mut(), Ordering::AcqRel),
        };
        unsafe {
            bag::store(
                object_pointer(_restore.global.get_nanbox_f64()).unwrap() as usize,
                COMPONENT_GUEST,
                undefined(),
            );
            assert_eq!(js_component_set_guest_ops(&ops), 1);
        }
        let outcome = crate::exception::catch_js_throw(|| {
            let text = |text: &[u8]| {
                scope.root_nanbox_f64(crate::value::js_nanbox_string(
                    crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32) as i64,
                ))
            };
            let interface = text(b"");
            let function = text(b"echo");
            let value = text("called before register 🌍".as_bytes());
            let args = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::array::js_array_alloc_with_length(1) as i64,
            ));
            crate::array::js_array_set_f64(
                array_pointer(args.get_nanbox_f64()).unwrap(),
                0,
                value.get_nanbox_f64(),
            );
            let output = js_component_import(
                interface.get_nanbox_f64(),
                function.get_nanbox_f64(),
                args.get_nanbox_f64(),
            );
            assert_eq!(string(output), "called before register 🌍");
            let contract = text(CONTRACT);
            let exports = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::object::js_object_alloc(0, 0) as i64,
            ));
            js_component_register(contract.get_nanbox_f64(), exports.get_nanbox_f64());
            assert!(crate::exception::catch_js_throw(|| js_component_register(
                contract.get_nanbox_f64(),
                exports.get_nanbox_f64()
            ))
            .is_err());
        });
        assert!(
            outcome.is_ok(),
            "initialization import or registration threw"
        );
    });
}

unsafe extern "C" fn unused_resource(_: *mut std::ffi::c_void, _: u32, _: u32, _: u32) -> u32 {
    0
}
