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
