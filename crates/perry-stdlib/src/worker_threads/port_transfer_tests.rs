use super::*;

pub(super) fn call(port: f64, method: &str, args: &[f64]) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let port = scope.root_nanbox_f64(port);
    let args: Vec<_> = args
        .iter()
        .map(|value| scope.root_nanbox_f64(*value))
        .collect();
    let method = scope.root_nanbox_f64(get_object_field_from_value(port.get_nanbox_f64(), method));
    let args: Vec<_> = args.iter().map(|value| value.get_nanbox_f64()).collect();
    unsafe {
        perry_runtime::closure::js_native_call_value(
            method.get_nanbox_f64(),
            perry_runtime::closure::JsThis::from_f64(port.get_nanbox_f64()),
            args.as_ptr(),
            args.len(),
        )
    }
}
fn channel() -> (f64, f64) {
    let channel = js_worker_threads_message_channel_new();
    (
        get_object_field_from_value(channel, "port1"),
        get_object_field_from_value(channel, "port2"),
    )
}
fn transfer_list(port: f64, buffer: Option<f64>) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let port = scope.root_nanbox_f64(port);
    let list = scope.root_raw_mut_ptr(perry_runtime::array::js_array_alloc(0));
    perry_runtime::array::js_array_push_f64(list.get_raw_mut_ptr(), port.get_nanbox_f64());
    if let Some(buffer) = buffer {
        perry_runtime::array::js_array_push_f64(list.get_raw_mut_ptr(), buffer);
    }
    object_value(
        list.get_raw_mut_ptr::<perry_runtime::array::ArrayHeader>()
            .cast(),
    )
}
fn snapshot(port: f64) -> SerializedValue {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let port = scope.root_nanbox_f64(port);
    let graph = scope.root_raw_mut_ptr(perry_runtime::object::js_object_alloc(0, 0));
    set_object_field(graph.get_raw_mut_ptr(), "port", port.get_nanbox_f64());
    set_object_field(graph.get_raw_mut_ptr(), "alias", port.get_nanbox_f64());
    set_object_field(
        graph.get_raw_mut_ptr(),
        "self",
        object_value(graph.get_raw_mut_ptr()),
    );
    let options = scope.root_raw_mut_ptr(perry_runtime::object::js_object_alloc(0, 0));
    set_object_field(
        options.get_raw_mut_ptr(),
        "workerData",
        object_value(graph.get_raw_mut_ptr()),
    );
    set_object_field(
        options.get_raw_mut_ptr(),
        "transferList",
        transfer_list(port.get_nanbox_f64(), None),
    );
    environment_data::prepare(object_value(options.get_raw_mut_ptr()))
}
fn received(port: f64) -> f64 {
    get_object_field_from_value(js_worker_threads_receive_message_on_port(port), "message")
}
fn isolated(test: impl FnOnce() + Send + 'static) {
    std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        perry_runtime::gc::gc_init();
        unsafe {
            crate::common::dispatch::js_stdlib_init_dispatch();
        }
        test();
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}

#[test]
fn worker_data_transfers_queued_ports_between_real_agents_and_back() {
    isolated(|| {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let (source, peer) = channel();
        let source = scope.root_nanbox_f64(source);
        let peer = scope.root_nanbox_f64(peer);
        call(peer.get_nanbox_f64(), "postMessage", &[7.0]);
        let payload = snapshot(source.get_nanbox_f64());
        let returned = std::thread::spawn(move || {
            let agent = perry_runtime::agent::enter_worker_agent();
            perry_runtime::gc::gc_init();
            environment_data::install(payload);
            let scope = perry_runtime::gc::RuntimeHandleScope::new();
            let data = scope.root_nanbox_f64(js_worker_threads_get_worker_data());
            let port =
                scope.root_nanbox_f64(get_object_field_from_value(data.get_nanbox_f64(), "port"));
            assert_eq!(
                port.get_nanbox_f64().to_bits(),
                get_object_field_from_value(data.get_nanbox_f64(), "alias").to_bits()
            );
            assert_eq!(
                data.get_nanbox_f64().to_bits(),
                get_object_field_from_value(data.get_nanbox_f64(), "self").to_bits()
            );
            assert_eq!(received(port.get_nanbox_f64()), 7.0);
            perry_runtime::gc::js_gc_collect();
            call(port.get_nanbox_f64(), "postMessage", &[42.0]);
            let returned = snapshot(port.get_nanbox_f64());
            perry_runtime::agent::retire_agent(agent);
            returned
        })
        .join()
        .unwrap();
        assert_eq!(received(peer.get_nanbox_f64()), 42.0);
        environment_data::install(returned);
        let data = js_worker_threads_get_worker_data();
        let latest = scope.root_nanbox_f64(get_object_field_from_value(data, "port"));
        assert_ne!(latest.get_nanbox_u64(), source.get_nanbox_u64());
        call(source.get_nanbox_f64(), "postMessage", &[99.0]);
        call(source.get_nanbox_f64(), "close", &[]);
        call(latest.get_nanbox_f64(), "postMessage", &[43.0]);
        assert_eq!(received(peer.get_nanbox_f64()), 43.0);
        assert!(is_undefined(js_worker_threads_receive_message_on_port(
            peer.get_nanbox_f64()
        )));
        call(latest.get_nanbox_f64(), "close", &[]);
    });
}

extern "C" fn throwing_getter(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    perry_runtime::exception::throw_program_value(
        perry_runtime::closure::js_closure_get_capture_f64(closure, 0),
    )
}
#[test]
fn getter_throw_keeps_buffer_ports_and_inbox_unchanged() {
    isolated(|| {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let (sender, receiver) = channel();
        let sender = scope.root_nanbox_f64(sender);
        let receiver = scope.root_nanbox_f64(receiver);
        let (source, peer) = channel();
        let source = scope.root_nanbox_f64(source);
        let peer = scope.root_nanbox_f64(peer);
        let buffer = scope.root_raw_mut_ptr(perry_runtime::buffer::js_array_buffer_new(8));
        let sentinel = scope.root_raw_mut_ptr(perry_runtime::object::js_object_alloc(0, 0));
        let captures = [object_value(sentinel.get_raw_mut_ptr()).to_bits()];
        let getter = scope.root_raw_mut_ptr(perry_runtime::closure::js_closure_alloc_init(
            perry_runtime::fn_info!(throwing_getter, 0; with_declared(0)),
            1,
            captures.as_ptr(),
        ));
        let graph = scope.root_raw_mut_ptr(perry_runtime::object::js_object_alloc(0, 0));
        let key = string_value("value");
        perry_runtime::object::js_object_define_accessor(
            object_value(graph.get_raw_mut_ptr()),
            key,
            object_value(getter.get_raw_mut_ptr::<ClosureHeader>().cast()),
            js_undefined(),
        );
        let list = scope.root_nanbox_f64(transfer_list(
            source.get_nanbox_f64(),
            Some(object_value(
                buffer
                    .get_raw_mut_ptr::<perry_runtime::buffer::BufferHeader>()
                    .cast(),
            )),
        ));
        let thrown = perry_runtime::exception::catch_js_throw(|| {
            call(
                sender.get_nanbox_f64(),
                "postMessage",
                &[object_value(graph.get_raw_mut_ptr()), list.get_nanbox_f64()],
            )
        });
        assert_eq!(
            thrown.unwrap_err().to_bits(),
            object_value(sentinel.get_raw_mut_ptr()).to_bits()
        );
        assert_eq!(
            perry_runtime::buffer::js_buffer_length(buffer.get_raw_mut_ptr()),
            8
        );
        assert!(is_undefined(js_worker_threads_receive_message_on_port(
            receiver.get_nanbox_f64()
        )));
        call(source.get_nanbox_f64(), "postMessage", &[42.0]);
        assert_eq!(received(peer.get_nanbox_f64()), 42.0);
        call(source.get_nanbox_f64(), "close", &[]);
        call(sender.get_nanbox_f64(), "close", &[]);
    });
}
#[test]
fn unread_transfer_ticket_releases_an_endpoint_absent_from_data() {
    isolated(|| {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let (sender, receiver) = channel();
        let sender = scope.root_nanbox_f64(sender);
        let receiver = scope.root_nanbox_f64(receiver);
        let (source, peer) = channel();
        let source = scope.root_nanbox_f64(source);
        let peer = scope.root_nanbox_f64(peer);
        let lease = perry_runtime::messaging::port_transfer::lookup(peer.get_nanbox_f64()).unwrap();
        let list = transfer_list(source.get_nanbox_f64(), None);
        call(sender.get_nanbox_f64(), "postMessage", &[js_null(), list]);
        assert!(port_transport::alive(lease));
        call(receiver.get_nanbox_f64(), "close", &[]);
        assert!(port_transport::closed(lease));
    });
}

static EXIT_MESSAGE: AtomicU64 = AtomicU64::new(0);
extern "C" fn exit_after_reply() {
    perry_runtime::gc::gc_init();
    js_worker_threads_post_message(42.0, js_undefined());
}
extern "C" fn record_exit_reply(
    _closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    value: f64,
) -> f64 {
    EXIT_MESSAGE.store(value.to_bits(), Ordering::Relaxed);
    js_undefined()
}
#[test]
fn worker_retirement_preserves_its_already_queued_parent_reply() {
    isolated(|| {
        EXIT_MESSAGE.store(0, Ordering::Relaxed);
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let worker = scope.root_nanbox_f64(js_worker_threads_worker_new(
            exit_after_reply as *const () as i64,
            js_undefined(),
        ));
        let id = perry_runtime::messaging::worker_id(worker.get_nanbox_f64()).unwrap();
        let callback = scope.root_raw_mut_ptr(perry_runtime::closure::js_closure_alloc(
            perry_runtime::fn_info!(record_exit_reply, 1; with_declared(1)),
            0,
        ));
        worker_add_listener(
            id,
            string_value("message"),
            object_value(callback.get_raw_mut_ptr::<ClosureHeader>().cast()),
            false,
            false,
        );
        let mut exited = false;
        for _ in 0..1000 {
            exited =
                PARENT_EVENTS.lock().unwrap().iter().any(
                    |(_, event)| matches!(event, WorkerEvent::Exit(worker, _) if *worker == id),
                );
            if exited {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(
            exited,
            "worker must retire before the parent drains its reply"
        );
        js_worker_threads_process_pending();
        assert_eq!(EXIT_MESSAGE.load(Ordering::Relaxed), 42.0f64.to_bits());
    });
}

static RETIRED_ORDER: AtomicU64 = AtomicU64::new(0);
extern "C" fn retired_message(
    _closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    value: f64,
) -> f64 {
    let order = RETIRED_ORDER.load(Ordering::Relaxed);
    RETIRED_ORDER.store(order * 10 + value as u64, Ordering::Relaxed);
    js_undefined()
}
extern "C" fn retired_close(
    _closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    let order = RETIRED_ORDER.load(Ordering::Relaxed);
    RETIRED_ORDER.store(order * 10 + 9, Ordering::Relaxed);
    js_undefined()
}
#[test]
fn ordinary_transferred_replies_survive_sender_close_and_retirement() {
    isolated(|| {
        for close in [false, true] {
            for asynchronous in [false, true] {
                RETIRED_ORDER.store(0, Ordering::Relaxed);
                let scope = perry_runtime::gc::RuntimeHandleScope::new();
                let (source, peer) = channel();
                let source = scope.root_nanbox_f64(source);
                let peer = scope.root_nanbox_f64(peer);
                if asynchronous {
                    for (name, info) in [
                        (
                            "message",
                            perry_runtime::fn_info!(retired_message, 1; with_declared(1)),
                        ),
                        (
                            "close",
                            perry_runtime::fn_info!(retired_close, 0; with_declared(0)),
                        ),
                    ] {
                        let callback = scope
                            .root_raw_mut_ptr(perry_runtime::closure::js_closure_alloc(info, 0));
                        call(
                            peer.get_nanbox_f64(),
                            "on",
                            &[
                                string_value(name),
                                object_value(callback.get_raw_mut_ptr::<ClosureHeader>().cast()),
                            ],
                        );
                    }
                }
                let payload = snapshot(source.get_nanbox_f64());
                std::thread::spawn(move || {
                    let agent = perry_runtime::agent::enter_worker_agent();
                    perry_runtime::gc::gc_init();
                    environment_data::install(payload);
                    let data = js_worker_threads_get_worker_data();
                    let scope = perry_runtime::gc::RuntimeHandleScope::new();
                    let port = scope.root_nanbox_f64(get_object_field_from_value(data, "port"));
                    call(port.get_nanbox_f64(), "postMessage", &[1.0]);
                    call(port.get_nanbox_f64(), "postMessage", &[2.0]);
                    if close {
                        call(port.get_nanbox_f64(), "close", &[]);
                    }
                    perry_runtime::agent::retire_agent(agent);
                })
                .join()
                .unwrap();
                // Joining proves the sender retired before either drain path.
                if asynchronous {
                    js_worker_threads_channels_process_pending();
                    js_worker_threads_channels_process_pending();
                    assert_eq!(
                        RETIRED_ORDER.load(Ordering::Relaxed),
                        129,
                        "two messages must precede close (explicit close={close})"
                    );
                } else {
                    assert_eq!(received(peer.get_nanbox_f64()), 1.0, "sender close={close}");
                    assert_eq!(received(peer.get_nanbox_f64()), 2.0);
                    assert!(is_undefined(js_worker_threads_receive_message_on_port(
                        peer.get_nanbox_f64()
                    )));
                }
                call(peer.get_nanbox_f64(), "close", &[]);
            }
        }
    });
}

static CLOSING_ORDER: AtomicU64 = AtomicU64::new(0);
static CLOSING_PEER: AtomicU64 = AtomicU64::new(0);
static CLOSING_SYNC_RECEIVE: AtomicU64 = AtomicU64::new(0);

extern "C" fn close_while_receiving(
    _closure: *const ClosureHeader,
    this: perry_runtime::closure::JsThis,
    value: f64,
) -> f64 {
    let order = CLOSING_ORDER.load(Ordering::Relaxed);
    CLOSING_ORDER.store(order * 10 + value as u64, Ordering::Relaxed);
    if value == 1.0 {
        let port = this.as_f64();
        call(port, "close", &[]);
        assert!(
            perry_runtime::messaging::port_transfer::prepare(transfer_list(port, None), None,)
                .is_err()
        );
        if CLOSING_SYNC_RECEIVE.load(Ordering::Relaxed) != 0 {
            assert_eq!(received(port), 2.0);
        }
        call(
            f64::from_bits(CLOSING_PEER.load(Ordering::Relaxed)),
            "postMessage",
            &[4.0],
        );
    }
    js_undefined()
}

extern "C" fn record_closing_order(
    _closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    let order = CLOSING_ORDER.load(Ordering::Relaxed);
    CLOSING_ORDER.store(order * 10 + 9, Ordering::Relaxed);
    js_undefined()
}

#[test]
fn closing_dispatch_drains_the_shared_inbox_before_close_but_cannot_transfer() {
    isolated(|| {
        for (outside, synchronous, expected) in
            [(true, false, 9), (false, false, 12349), (false, true, 1349)]
        {
            CLOSING_ORDER.store(0, Ordering::Relaxed);
            CLOSING_SYNC_RECEIVE.store(u64::from(synchronous), Ordering::Relaxed);
            let scope = perry_runtime::gc::RuntimeHandleScope::new();
            let (port, peer) = channel();
            let port = scope.root_nanbox_f64(port);
            let peer = scope.root_nanbox_f64(peer);
            CLOSING_PEER.store(peer.get_nanbox_u64(), Ordering::Relaxed);
            for (event, info) in [
                (
                    "message",
                    perry_runtime::fn_info!(close_while_receiving, 1; with_declared(1)),
                ),
                (
                    "close",
                    perry_runtime::fn_info!(record_closing_order, 0; with_declared(0)),
                ),
            ] {
                let callback =
                    scope.root_raw_mut_ptr(perry_runtime::closure::js_closure_alloc(info, 0));
                call(
                    port.get_nanbox_f64(),
                    "on",
                    &[
                        string_value(event),
                        object_value(callback.get_raw_mut_ptr::<ClosureHeader>().cast()),
                    ],
                );
            }
            for value in [1.0, 2.0, 3.0] {
                call(peer.get_nanbox_f64(), "postMessage", &[value]);
            }
            if outside {
                call(port.get_nanbox_f64(), "close", &[]);
            }
            js_worker_threads_channels_process_pending();
            assert_eq!(CLOSING_ORDER.load(Ordering::Relaxed), expected);
            assert!(is_undefined(js_worker_threads_receive_message_on_port(
                port.get_nanbox_f64()
            )));
            js_worker_threads_channels_process_pending();
            assert_eq!(
                CLOSING_ORDER.load(Ordering::Relaxed),
                expected,
                "close fires once"
            );
            call(peer.get_nanbox_f64(), "close", &[]);
        }
    });
}

#[test]
fn port_listener_admission_rejects_primitives_without_changing_valid_delivery() {
    isolated(|| {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let (port, peer) = channel();
        let port = scope.root_nanbox_f64(port);
        let peer = scope.root_nanbox_f64(peer);
        let event = scope.root_nanbox_f64(string_value("message"));
        let callback = scope.root_raw_mut_ptr(perry_runtime::closure::js_closure_alloc(
            perry_runtime::fn_info!(retired_message, 1; with_declared(1)),
            0,
        ));
        call(
            port.get_nanbox_f64(),
            "on",
            &[
                event.get_nanbox_f64(),
                object_value(callback.get_raw_mut_ptr::<ClosureHeader>().cast()),
            ],
        );
        for method in [
            "on",
            "once",
            "addListener",
            "off",
            "removeListener",
            "addEventListener",
            "removeEventListener",
        ] {
            let caught = perry_runtime::exception::catch_thrown(|| {
                call(
                    port.get_nanbox_f64(),
                    method,
                    &[event.get_nanbox_f64(), 1.0],
                )
            })
            .expect_err(method);
            let error = scope.root_nanbox_f64(caught.value());
            for (key, expected) in [
                ("name", "TypeError"),
                ("code", "ERR_INVALID_ARG_TYPE"),
                ("message", "The \"listener\" argument must be an instance of EventListener. Received type number (1)"),
            ] {
                assert_eq!(string_value_to_string(get_object_field_from_value(error.get_nanbox_f64(), key)).as_deref(), Some(expected), "{method}: {key}");
            }
            for listener in [
                js_null(),
                js_undefined(),
                object_value(
                    scope
                        .root_raw_mut_ptr(perry_runtime::object::js_object_alloc(0, 0))
                        .get_raw_mut_ptr(),
                ),
            ] {
                call(
                    port.get_nanbox_f64(),
                    method,
                    &[event.get_nanbox_f64(), listener],
                );
            }
        }
        RETIRED_ORDER.store(0, Ordering::Relaxed);
        call(peer.get_nanbox_f64(), "postMessage", &[7.0]);
        js_worker_threads_channels_process_pending();
        assert_eq!(RETIRED_ORDER.load(Ordering::Relaxed), 7);
        call(port.get_nanbox_f64(), "close", &[]);
        call(peer.get_nanbox_f64(), "close", &[]);
    });
}
