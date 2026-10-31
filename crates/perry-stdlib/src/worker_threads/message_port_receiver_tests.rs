use super::*;

#[test]
fn duplicate_private_id_cannot_impersonate_registered_port_identity() {
    std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        perry_runtime::gc::gc_init();
        ensure_environment_data_gc_scanner();
        let id = 999;
        MESSAGE_PORTS.with(|ports| {
            ports.borrow_mut().insert(id, MessagePortState::default());
        });
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let port = scope.root_raw_mut_ptr(message_port::message_port_object(id));
        let impostor =
            scope.root_raw_mut_ptr(perry_runtime::messaging::message_port_object_with_id(id));
        let mut result = js_undefined();
        let invoke = |receiver, result: &mut f64| unsafe {
            message_port::dispatch_core_operation(
                perry_runtime::messaging::MessagePortOperation::Ref,
                receiver,
                js_undefined(),
                js_undefined(),
                result,
            )
        };
        assert!(!invoke(
            object_value(impostor.get_raw_mut_ptr()),
            &mut result
        ));
        assert!(!message_port::port_is_refed(id));
        assert!(invoke(object_value(port.get_raw_mut_ptr()), &mut result));
        assert!(message_port::port_is_refed(id));
        MESSAGE_PORTS.with(|ports| {
            ports.borrow_mut().remove(&id);
        });
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}

extern "C" fn close_listener_probe(
    _closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    js_undefined()
}

#[test]
fn close_callback_uses_existing_once_listener_storage() {
    std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        perry_runtime::gc::gc_init();
        ensure_environment_data_gc_scanner();
        let id = 998;
        MESSAGE_PORTS.with(|ports| {
            ports.borrow_mut().insert(id, MessagePortState::default());
        });
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let port = scope.root_raw_mut_ptr(message_port::message_port_object(id));
        let callback = perry_runtime::closure::js_closure_alloc(
            perry_runtime::fn_info!(close_listener_probe, 0; with_declared(0)),
            0,
        );
        let callback =
            scope.root_nanbox_f64(perry_runtime::value::js_nanbox_pointer(callback as i64));
        for _ in 0..2 {
            let mut result = js_undefined();
            assert!(unsafe {
                message_port::dispatch_core_operation(
                    perry_runtime::messaging::MessagePortOperation::Close,
                    object_value(port.get_raw_mut_ptr()),
                    callback.get_nanbox_f64(),
                    js_undefined(),
                    &mut result,
                )
            });
            assert_eq!(result.to_bits(), js_undefined().to_bits());
        }
        MESSAGE_PORTS.with(|ports| {
            let mut ports = ports.borrow_mut();
            let state = ports.remove(&id).unwrap();
            assert!(state.closed && state.close_pending);
            assert_eq!(state.close_cbs.len(), 1);
            assert!(state.close_cbs[0].once);
            assert_eq!(
                state.close_cbs[0].callback_bits,
                callback.get_nanbox_f64().to_bits()
            );
        });
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}
