use super::*;

fn field(value: f64, name: &str) -> f64 {
    object::js_object_get_field_by_name_f64(
        crate::value::js_nanbox_get_pointer(value) as *const ObjectHeader,
        key(name),
    )
}
fn call(port: f64, name: &str, args: &[f64]) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let port = scope.root_nanbox_f64(port);
    let method = scope.root_nanbox_f64(field(port.get_nanbox_f64(), name));
    unsafe {
        crate::closure::js_native_call_value(
            method.get_nanbox_f64(),
            crate::closure::JsThis::from_f64(port.get_nanbox_f64()),
            args.as_ptr(),
            args.len(),
        )
    }
}
fn pair() -> (f64, f64) {
    let channel = same_thread_message_channel_new_for_test();
    (field(channel, "port1"), field(channel, "port2"))
}
fn packet(carrier: f64, value: f64, port: f64, receiver: f64) -> crate::thread::SerializedValue {
    let scope = crate::gc::RuntimeHandleScope::new();
    let carrier = scope.root_nanbox_f64(carrier);
    let value = scope.root_nanbox_f64(value);
    let port = scope.root_nanbox_f64(port);
    let list = scope.root_raw_mut_ptr(crate::array::js_array_alloc(0));
    crate::array::js_array_push_f64(list.get_raw_mut_ptr(), port.get_nanbox_f64());
    call(
        carrier.get_nanbox_f64(),
        "postMessage",
        &[
            value.get_nanbox_f64(),
            boxed_object(list.get_raw_mut_ptr::<crate::array::ArrayHeader>().cast()),
        ],
    );
    port_transport::receive(runtime_port_lease(receiver).unwrap()).unwrap()
}

#[test]
fn runtime_port_transfer_crosses_real_agents_and_never_reactivates_old_wrappers() {
    std::thread::spawn(|| {
        let agent = crate::agent::enter_worker_agent();
        crate::gc::gc_init();
        let scope = crate::gc::RuntimeHandleScope::new();
        let (source, peer) = pair();
        let source = scope.root_nanbox_f64(source);
        let peer = scope.root_nanbox_f64(peer);
        let (carrier, receiver) = pair();
        let carrier = scope.root_nanbox_f64(carrier);
        let receiver = scope.root_nanbox_f64(receiver);
        call(peer.get_nanbox_f64(), "postMessage", &[7.0]);
        let message = packet(
            carrier.get_nanbox_f64(),
            source.get_nanbox_f64(),
            source.get_nanbox_f64(),
            receiver.get_nanbox_f64(),
        );
        let foreign_identity = source.get_nanbox_u64();
        let returned = std::thread::spawn(move || {
            let agent = crate::agent::enter_worker_agent();
            crate::gc::gc_init();
            let scope = crate::gc::RuntimeHandleScope::new();
            let adopted = scope.root_nanbox_u64(unsafe {
                crate::thread::deserialize_nanbox_on_current_thread(&message)
            });
            assert_eq!(message_port_id(adopted.get_nanbox_f64()), Some(0));
            assert_eq!(
                take_runtime_port_message(adopted.get_nanbox_f64()),
                Some(7.0)
            );
            let mut visited = Vec::new();
            port_states_root_scanner_mut(&mut crate::gc::RuntimeRootVisitor::for_copy(
                &mut |value: f64| {
                    visited.push(value.to_bits());
                },
            ));
            assert!(!visited.contains(&foreign_identity));
            assert!(visited.contains(&adopted.get_nanbox_u64()));
            crate::gc::js_gc_collect();
            call(adopted.get_nanbox_f64(), "postMessage", &[42.0]);
            let (carrier, receiver) = pair();
            let message = packet(
                carrier,
                adopted.get_nanbox_f64(),
                adopted.get_nanbox_f64(),
                receiver,
            );
            crate::agent::retire_agent(agent);
            message
        })
        .join()
        .unwrap();
        assert_eq!(take_runtime_port_message(peer.get_nanbox_f64()), Some(42.0));
        let latest = scope.root_nanbox_u64(unsafe {
            crate::thread::deserialize_nanbox_on_current_thread(&returned)
        });
        assert_ne!(source.get_nanbox_u64(), latest.get_nanbox_u64());
        call(source.get_nanbox_f64(), "postMessage", &[99.0]);
        call(source.get_nanbox_f64(), "close", &[]);
        call(latest.get_nanbox_f64(), "postMessage", &[43.0]);
        assert_eq!(take_runtime_port_message(peer.get_nanbox_f64()), Some(43.0));
        assert_eq!(take_runtime_port_message(peer.get_nanbox_f64()), None);
        call(latest.get_nanbox_f64(), "close", &[]);
        crate::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}

extern "C" fn transfer_buffer_in_getter(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let buffer = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    let holder = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 1));
    let address = crate::value::js_nanbox_get_pointer(buffer.get_nanbox_f64()) as usize;
    let message = unsafe {
        crate::thread::serialize_message(buffer.get_nanbox_u64(), &[address], None).unwrap()
    };
    let adopted = scope
        .root_nanbox_u64(unsafe { crate::thread::deserialize_nanbox_on_current_thread(&message) });
    set_field(
        crate::value::js_nanbox_get_pointer(holder.get_nanbox_f64()) as *mut ObjectHeader,
        "inner",
        adopted.get_nanbox_f64(),
    );
    1.0
}

#[test]
fn getter_buffer_transfer_rejects_outer_port_commit_without_undoing_inner_transfer() {
    std::thread::spawn(|| {
        let agent = crate::agent::enter_worker_agent();
        crate::gc::gc_init();
        let scope = crate::gc::RuntimeHandleScope::new();
        let (sender, receiver) = pair();
        let sender = scope.root_nanbox_f64(sender);
        let receiver = scope.root_nanbox_f64(receiver);
        let (source, peer) = pair();
        let source = scope.root_nanbox_f64(source);
        let peer = scope.root_nanbox_f64(peer);
        let lease = runtime_port_lease(source.get_nanbox_f64()).unwrap();
        let buffer = scope.root_raw_mut_ptr(crate::buffer::js_array_buffer_new(8));
        crate::buffer::js_buffer_set(buffer.get_raw_mut_ptr(), 0, 7);
        let holder = scope.root_raw_mut_ptr(object::js_object_alloc(0, 0));
        let captures = [
            boxed_object(
                buffer
                    .get_raw_mut_ptr::<crate::buffer::BufferHeader>()
                    .cast(),
            )
            .to_bits(),
            boxed_object(holder.get_raw_mut_ptr()).to_bits(),
        ];
        let getter = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc_init(
            crate::fn_info!(transfer_buffer_in_getter, 0; with_declared(0)),
            2,
            captures.as_ptr(),
        ));
        let graph = scope.root_raw_mut_ptr(object::js_object_alloc(0, 0));
        let name = crate::value::js_nanbox_string(key("move") as i64);
        object::js_object_define_accessor(
            boxed_object(graph.get_raw_mut_ptr()),
            name,
            crate::value::js_nanbox_pointer(getter.get_raw_mut_ptr::<ClosureHeader>() as i64),
            js_undefined(),
        );
        let list = scope.root_raw_mut_ptr(crate::array::js_array_alloc(0));
        crate::array::js_array_push_f64(list.get_raw_mut_ptr(), source.get_nanbox_f64());
        crate::array::js_array_push_f64(
            list.get_raw_mut_ptr(),
            crate::value::js_nanbox_pointer(
                buffer.get_raw_mut_ptr::<crate::buffer::BufferHeader>() as i64,
            ),
        );
        let result = crate::exception::catch_js_throw(|| {
            call(
                sender.get_nanbox_f64(),
                "postMessage",
                &[
                    boxed_object(graph.get_raw_mut_ptr()),
                    crate::value::js_nanbox_pointer(
                        list.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64,
                    ),
                ],
            )
        });
        let inner = scope.root_nanbox_f64(field(boxed_object(holder.get_raw_mut_ptr()), "inner"));
        let inner = crate::value::js_nanbox_get_pointer(inner.get_nanbox_f64())
            as *const crate::buffer::BufferHeader;
        let root_state = (
            result.is_err(),
            crate::buffer::js_buffer_length(buffer.get_raw_mut_ptr()),
            crate::buffer::js_buffer_length(inner),
            crate::buffer::js_buffer_get(inner, 0),
            port_transport::alive(lease),
            port_transport::pending(runtime_port_lease(receiver.get_nanbox_f64()).unwrap()),
        );
        assert_eq!(
            root_state,
            (true, 0, 8, 7, true, false),
            "RootState: outer rejected, inner buffer retained, original port owned, carrier empty"
        );
        call(source.get_nanbox_f64(), "postMessage", &[42.0]);
        assert_eq!(take_runtime_port_message(peer.get_nanbox_f64()), Some(42.0));
        crate::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}

#[test]
fn runtime_transit_ticket_keeps_disentangled_identity_after_empty_receive() {
    std::thread::spawn(|| {
        let agent = crate::agent::enter_worker_agent();
        crate::gc::gc_init();
        for queued in [false, true] {
            let scope = crate::gc::RuntimeHandleScope::new();
            let (source, peer) = pair();
            let source = scope.root_nanbox_f64(source);
            let peer = scope.root_nanbox_f64(peer);
            let (sender, receiver) = pair();
            let sender = scope.root_nanbox_f64(sender);
            let receiver = scope.root_nanbox_f64(receiver);
            if queued {
                call(peer.get_nanbox_f64(), "postMessage", &[7.0]);
            }
            let list = scope.root_raw_mut_ptr(crate::array::js_array_alloc(0));
            crate::array::js_array_push_f64(list.get_raw_mut_ptr(), source.get_nanbox_f64());
            call(
                sender.get_nanbox_f64(),
                "postMessage",
                &[
                    source.get_nanbox_f64(),
                    crate::value::js_nanbox_pointer(
                        list.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64,
                    ),
                ],
            );
            call(peer.get_nanbox_f64(), "close", &[]);
            let message =
                port_transport::receive(runtime_port_lease(receiver.get_nanbox_f64()).unwrap())
                    .unwrap();
            std::thread::spawn(move || {
                let agent = crate::agent::enter_worker_agent();
                crate::gc::gc_init();
                let scope = crate::gc::RuntimeHandleScope::new();
                let adopted = scope.root_nanbox_u64(unsafe {
                    crate::thread::deserialize_nanbox_on_current_thread(&message)
                });
                assert_eq!(message_port_id(adopted.get_nanbox_f64()), Some(0));
                assert_eq!(
                    take_runtime_port_message(adopted.get_nanbox_f64()),
                    queued.then_some(7.0)
                );
                assert_eq!(take_runtime_port_message(adopted.get_nanbox_f64()), None);
                assert_eq!(
                    call(adopted.get_nanbox_f64(), "hasRef", &[]).to_bits(),
                    js_bool(false).to_bits()
                );
                call(adopted.get_nanbox_f64(), "ref", &[]);
                assert_eq!(
                    call(adopted.get_nanbox_f64(), "hasRef", &[]).to_bits(),
                    js_bool(true).to_bits()
                );
                assert_eq!(
                    call(adopted.get_nanbox_f64(), "postMessage", &[8.0]).to_bits(),
                    js_bool(true).to_bits()
                );
                assert_eq!(message_port_id(adopted.get_nanbox_f64()), Some(0));
                call(adopted.get_nanbox_f64(), "close", &[]);
                crate::agent::retire_agent(agent);
            })
            .join()
            .unwrap();
        }
        crate::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}
