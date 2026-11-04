use super::*;

#[test]
fn synchronous_receive_checks_identity_and_accepts_runtime_closed_brand() {
    std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        perry_runtime::gc::gc_init();
        ensure_environment_data_gc_scanner();
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let port = scope.root_raw_mut_ptr(perry_runtime::messaging::message_port_object_with_id(0));
        assert_eq!(
            js_worker_threads_receive_message_on_port(object_value(port.get_raw_mut_ptr()))
                .to_bits(),
            js_undefined().to_bits()
        );
        let id = 997;
        MESSAGE_PORTS.with(|ports| {
            ports.borrow_mut().insert(id, MessagePortState::default());
        });
        let genuine = scope.root_raw_mut_ptr(message_port::message_port_object(id));
        let impostor =
            scope.root_raw_mut_ptr(perry_runtime::messaging::message_port_object_with_id(id));
        assert!(perry_runtime::exception::catch_js_throw(|| {
            js_worker_threads_receive_message_on_port(object_value(impostor.get_raw_mut_ptr()))
        })
        .is_err());
        assert_eq!(
            js_worker_threads_receive_message_on_port(object_value(genuine.get_raw_mut_ptr()))
                .to_bits(),
            js_undefined().to_bits()
        );
        MESSAGE_PORTS.with(|ports| {
            ports.borrow_mut().remove(&id);
        });
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}
