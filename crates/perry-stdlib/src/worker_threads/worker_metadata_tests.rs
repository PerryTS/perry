use super::*;
use perry_runtime::gc::RuntimeHandleScope;

extern "C" {
    fn js_shape_run_static_seed();
}

fn read(worker: f64, key: &str) -> f64 {
    let scope = RuntimeHandleScope::new();
    let worker = scope.root_nanbox_f64(worker);
    let key = string_value(key);
    unsafe { perry_runtime::object::js_object_get_property_key(worker.get_nanbox_f64(), key) }
}

extern "C" fn on_exit(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    _code: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let worker = scope.root_nanbox_f64(perry_runtime::closure::js_closure_get_capture_f64(
        closure, 0,
    ));
    assert_eq!(read(worker.get_nanbox_f64(), "threadId"), -1.0);
    assert_eq!(
        read(worker.get_nanbox_f64(), "threadName").to_bits(),
        JSValue::null().bits()
    );
    let limits = read(worker.get_nanbox_f64(), "resourceLimits");
    assert!(is_undefined(read(limits, "stackSizeMb")));
    js_undefined()
}

#[test]
fn metadata_getters_follow_owner_and_exit_pump() {
    std::thread::spawn(|| {
        perry_runtime::agent::enter_worker_agent();
        perry_runtime::gc::gc_init();
        unsafe {
            js_shape_run_static_seed();
        }
        let scope = RuntimeHandleScope::new();
        let placeholder = scope.root_raw_mut_ptr(perry_runtime::object::js_object_alloc(0, 0));
        let id = thread_exit_probe::insert_worker_for_test(
            object_value(placeholder.get_raw_mut_ptr()).to_bits(),
            JSValue::undefined().bits(),
        );
        let worker =
            scope.root_raw_mut_ptr(worker_surface::worker_object(id, &WorkerOptions::default()));
        let value = || object_value(worker.get_raw_mut_ptr());
        WORKERS.lock().unwrap().get_mut(&id).unwrap().object_bits = value().to_bits();
        let constructor =
            scope.root_nanbox_f64(perry_runtime::object::bound_native_callable_export_value(
                &perry_runtime::object::NM_WORKER_THREADS,
                "worker_threads",
                "Worker",
            ));
        assert_eq!(
            perry_runtime::object::js_object_get_prototype_of(value()).to_bits(),
            perry_runtime::object::js_function_prototype_value_for_read(
                constructor.get_nanbox_f64()
            )
            .to_bits()
        );
        assert_eq!(read(value(), "threadId"), id as f64);
        assert_eq!(
            string_value_to_string(read(value(), "threadName")).as_deref(),
            Some("metadata-test")
        );
        let limits = scope.root_nanbox_f64(read(value(), "resourceLimits"));
        set_object_field(
            JSValue::from_bits(limits.get_nanbox_f64().to_bits())
                .as_pointer::<perry_runtime::ObjectHeader>()
                .cast_mut(),
            "stackSizeMb",
            77.0,
        );
        let next = scope.root_nanbox_f64(read(value(), "resourceLimits"));
        assert_ne!(
            limits.get_nanbox_f64().to_bits(),
            next.get_nanbox_f64().to_bits()
        );
        assert_eq!(read(next.get_nanbox_f64(), "stackSizeMb"), 4.0);
        let foreign = std::thread::spawn(move || {
            perry_runtime::agent::enter_worker_agent();
            perry_runtime::gc::gc_init();
            worker_surface::worker_metadata(
                id,
                perry_runtime::messaging::WorkerMetadataField::ThreadId,
            )
        })
        .join()
        .unwrap();
        assert!(is_undefined(foreign));
        let captures = [value().to_bits()];
        let callback = perry_runtime::closure::js_closure_alloc_init(
            perry_runtime::fn_info!(on_exit, 1; with_declared(1)),
            1,
            captures.as_ptr(),
        );
        worker_add_listener(
            id,
            string_value("exit"),
            object_value(callback.cast()),
            false,
            false,
        );
        push_parent_event(
            perry_runtime::agent::current_agent(),
            WorkerEvent::Exit(id, 0),
        );
        assert!(js_worker_threads_process_pending() > 0);
        assert_eq!(read(value(), "threadId"), -1.0);
        let first = scope.root_nanbox_f64(read(value(), "resourceLimits"));
        let second = scope.root_nanbox_f64(read(value(), "resourceLimits"));
        assert_ne!(
            first.get_nanbox_f64().to_bits(),
            second.get_nanbox_f64().to_bits()
        );
        WORKERS.lock().unwrap().remove(&id);
    })
    .join()
    .unwrap();
}

#[test]
fn exit_keeps_own_metadata_shadows_and_private_receiver_identity() {
    std::thread::spawn(|| {
        perry_runtime::agent::enter_worker_agent();
        perry_runtime::gc::gc_init();
        unsafe {
            js_shape_run_static_seed();
        }
        let scope = RuntimeHandleScope::new();
        let object = scope.root_raw_mut_ptr(perry_runtime::object::js_object_alloc(0, 0));
        let id = thread_exit_probe::insert_worker_for_test(
            object_value(object.get_raw_mut_ptr()).to_bits(),
            JSValue::undefined().bits(),
        );
        let worker =
            scope.root_raw_mut_ptr(worker_surface::worker_object(id, &WorkerOptions::default()));
        let value = || object_value(worker.get_raw_mut_ptr());
        WORKERS.lock().unwrap().get_mut(&id).unwrap().object_bits = value().to_bits();
        for (key, shadow) in [
            ("threadId", id as f64),
            ("threadName", 43.0),
            ("resourceLimits", 44.0),
        ] {
            let descriptor = scope.root_raw_mut_ptr(perry_runtime::object::js_object_alloc(0, 0));
            set_object_field(descriptor.get_raw_mut_ptr(), "value", shadow);
            let key = string_value(key);
            perry_runtime::object::js_object_define_property(
                value(),
                key,
                object_value(descriptor.get_raw_mut_ptr()),
            );
        }
        assert_eq!(
            worker_surface::worker_id_from_receiver(
                worker.get_raw_mut_ptr::<perry_runtime::ObjectHeader>() as i64
            ),
            Some(id)
        );
        push_parent_event(
            perry_runtime::agent::current_agent(),
            WorkerEvent::Exit(id, 0),
        );
        assert!(js_worker_threads_process_pending() > 0);
        assert_eq!(read(value(), "threadId"), id as f64);
        assert_eq!(read(value(), "threadName"), 43.0);
        assert_eq!(read(value(), "resourceLimits"), 44.0);
        WORKERS.lock().unwrap().remove(&id);
    })
    .join()
    .unwrap();
}
