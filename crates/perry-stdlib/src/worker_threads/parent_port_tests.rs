use super::*;
use std::sync::atomic::AtomicUsize;

static CALLS: AtomicUsize = AtomicUsize::new(0);

extern "C" fn listener(
    _closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    value: f64,
) -> f64 {
    assert_eq!(value, 42.0);
    // Removal happens before invocation: reentrant dispatch cannot repeat it.
    assert!(!message_port::port_is_refed(PARENT_PORT_HANDLE as u64));
    CALLS.fetch_add(1, Ordering::Relaxed);
    js_undefined()
}

fn isolated(test: impl FnOnce() + Send + 'static) {
    std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        perry_runtime::gc::gc_init();
        CURRENT_WORKER_ID.with(|id| id.set(u64::MAX));
        test();
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}

#[test]
fn parent_port_once_consumes_its_reference_before_calling_js() {
    isolated(|| {
        CALLS.store(0, Ordering::Relaxed);
        let port = object_value(parent_port::worker_parent_port_object());
        assert_eq!(
            port.to_bits(),
            object_value(parent_port::worker_parent_port_object()).to_bits()
        );
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let port = scope.root_nanbox_f64(port);
        let closure = perry_runtime::closure::js_closure_alloc(
            perry_runtime::fn_info!(listener, 1; with_declared(1)),
            0,
        );
        let callback =
            scope.root_nanbox_f64(perry_runtime::value::js_nanbox_pointer(closure as i64));
        message_port::add_node_listener(
            PARENT_PORT_HANDLE as u64,
            string_value("message"),
            callback.get_nanbox_f64(),
            true,
        );
        assert!(message_port::port_is_refed(PARENT_PORT_HANDLE as u64));
        assert_eq!(js_worker_threads_channels_has_pending(), 1);
        let mut roots = Vec::new();
        scan_environment_data_roots_mut(&mut perry_runtime::gc::RuntimeRootVisitor::for_copy(
            &mut |v: f64| roots.push(v.to_bits()),
        ));
        assert!(roots.contains(&port.get_nanbox_f64().to_bits()));
        assert!(roots.contains(&callback.get_nanbox_f64().to_bits()));
        deliver_parent_port_message(SerializedValue::Inline(42.0f64.to_bits()));
        deliver_parent_port_message(SerializedValue::Inline(42.0f64.to_bits()));
        assert_eq!(CALLS.load(Ordering::Relaxed), 1);
        assert_eq!(js_worker_threads_channels_has_pending(), 0);
    });
}

#[test]
fn parent_port_close_and_unref_use_the_message_port_handle_state() {
    isolated(|| {
        unsafe {
            crate::common::dispatch::js_stdlib_init_dispatch();
        }
        let port = object_value(parent_port::worker_parent_port_object());
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let port = scope.root_nanbox_f64(port);
        for (method, expected) in [
            ("ref", true),
            ("unref", false),
            ("ref", true),
            ("close", false),
        ] {
            let closure = get_object_field_from_value(port.get_nanbox_f64(), method);
            let closure =
                perry_runtime::value::js_nanbox_get_pointer(closure) as *const ClosureHeader;
            perry_runtime::closure::js_closure_call0(
                closure,
                perry_runtime::closure::JsThis::from_f64(port.get_nanbox_f64()),
            );
            assert_eq!(
                message_port::port_is_refed(PARENT_PORT_HANDLE as u64),
                expected
            );
        }
        assert!(parent_port::is_closed());
        assert_eq!(
            js_worker_threads_channels_has_pending(),
            1,
            "close is queued, not delivered inline"
        );
        assert_eq!(js_worker_threads_channels_process_pending(), 1);
        assert_eq!(js_worker_threads_channels_has_pending(), 0);
        deliver_parent_port_message(SerializedValue::Inline(42.0f64.to_bits()));
        MESSAGE_PORTS.with(|ports| {
            assert!(ports.borrow()[&(PARENT_PORT_HANDLE as u64)]
                .inbox
                .is_empty())
        });
    });
}

#[test]
fn a_worker_does_not_count_its_parents_worker_record_as_its_own_work() {
    isolated(|| {
        let (tx, _rx) = mpsc::channel();
        let id = NEXT_WORKER_ID.fetch_add(1, Ordering::Relaxed);
        let mut record = WorkerRecord {
            owner_agent: perry_runtime::agent::PRIMARY_AGENT,
            sender: WorkerSender::new(tx),
            object_bits: JSValue::undefined().bits(),
            listeners: HashMap::new(),
            alive: false,
            refed: false,
            terminate_promise: None,
            async_resources: [perry_runtime::async_hooks::AsyncResourceIds {
                async_id: 0,
                trigger_async_id: 0,
            }; 3],
            async_resource_bits: [JSValue::undefined().bits(); 3],
        };
        record.set_liveness(true, true);
        WORKERS.lock().unwrap().insert(id, record);
        assert_eq!(js_worker_threads_has_pending(), 0);
        let mut record = WORKERS.lock().unwrap().remove(&id).unwrap();
        record.set_liveness(false, false);
    });
}

#[test]
fn parent_events_are_counted_per_agent_and_drained_by_their_own_pump() {
    isolated(|| {
        let me = perry_runtime::agent::current_agent();
        let other = std::thread::spawn(|| {
            let agent = perry_runtime::agent::enter_worker_agent();
            with_own_counts(|_| ());
            agent
        })
        .join()
        .unwrap();
        with_own_counts(|_| ());
        assert_eq!(js_worker_threads_has_pending(), 0);
        push_parent_event(other, WorkerEvent::Online(0));
        assert_eq!(
            js_worker_threads_has_pending(),
            0,
            "another agent's event kept this agent alive"
        );
        push_parent_event(me, WorkerEvent::Online(0));
        assert_eq!(js_worker_threads_has_pending(), 1);
        assert_eq!(js_worker_threads_process_pending(), 1);
        assert_eq!(js_worker_threads_has_pending(), 0);
        assert_eq!(
            PARENT_EVENTS
                .lock()
                .unwrap()
                .iter()
                .filter(|(parent, _)| *parent == other)
                .count(),
            1,
            "this agent's pump took another agent's event"
        );
        // A retired parent's events are dropped rather than queued for ever.
        worker_pump::purge_parent_events_for_test(other);
        push_parent_event(other, WorkerEvent::Online(0));
        assert!(!PARENT_EVENTS
            .lock()
            .unwrap()
            .iter()
            .any(|(parent, _)| *parent == other));
    });
}
