use super::*;
use perry_runtime::agent::enter_worker_agent;
use perry_runtime::gc::{RuntimeHandleScope, RuntimeRootVisitor};

extern "C" fn callback(
    _closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    f64::from_bits(JSValue::undefined().bits())
}

fn emitted_roots() -> Vec<u64> {
    let mut words = Vec::new();
    let mut record = |value: f64| words.push(value.to_bits());
    scan_worker_roots_mut(&mut RuntimeRootVisitor::for_copy(&mut record));
    words
}

#[test]
fn worker_scanner_keeps_owner_roots_and_excludes_another_agents_heap() {
    std::thread::spawn(|| {
        let owner = enter_worker_agent();
        perry_runtime::gc::gc_init();
        let scope = RuntimeHandleScope::new();
        let object = perry_runtime::object::js_object_alloc(0, 0);
        let object = scope.root_nanbox_f64(perry_runtime::value::js_nanbox_pointer(object as i64));
        let closure = perry_runtime::closure::js_closure_alloc(
            perry_runtime::fn_info!(callback, 0; with_declared(0)),
            0,
        );
        let callback =
            scope.root_nanbox_f64(perry_runtime::value::js_nanbox_pointer(closure as i64));
        let object_bits = object.get_nanbox_f64().to_bits();
        let callback_bits = callback.get_nanbox_f64().to_bits();
        let worker = thread_exit_probe::insert_worker_for_test(object_bits, callback_bits);
        let first = scope.root_nanbox_f64(worker_terminate_by_id(worker));
        let second = scope.root_nanbox_f64(worker_terminate_by_id(worker));
        let owned = emitted_roots();
        let (foreign, other_agent) = std::thread::spawn(|| {
            let agent = enter_worker_agent();
            perry_runtime::gc::gc_init();
            (agent, emitted_roots())
        })
        .join()
        .unwrap();
        assert_ne!(foreign, owner);
        if let Some(mut record) = WORKERS.lock().unwrap().remove(&worker) {
            record.set_liveness(false, false);
        }
        assert!(owned.contains(&object_bits));
        assert!(owned.contains(&callback_bits));
        for promise in [&first, &second] {
            let bits = promise.get_nanbox_f64().to_bits();
            assert!(owned.contains(&bits));
            assert!(!other_agent.contains(&bits));
            let ptr = JSValue::from_bits(bits)
                .as_pointer::<perry_runtime::Promise>()
                .cast_mut();
            perry_runtime::js_promise_resolve(ptr, 0.0);
        }
        assert!(
            !other_agent.contains(&object_bits),
            "foreign Worker handle was emitted"
        );
        assert!(
            !other_agent.contains(&callback_bits),
            "foreign callback was emitted"
        );
    })
    .join()
    .unwrap();
}

#[test]
fn process_notification_callback_retains_its_worker_on_the_creating_threads_tick_queue() {
    std::thread::spawn(|| {
        enter_worker_agent();
        perry_runtime::gc::gc_init();
        let (callback_bits, worker_bits) = {
            let scope = RuntimeHandleScope::new();
            let worker = scope.root_raw_mut_ptr(perry_runtime::object::js_object_alloc(0, 0));
            let callback = scope.root_nanbox_f64(worker_surface::process_worker_callback(
                object_value(worker.get_raw_mut_ptr()),
            ));
            let ptr = || {
                JSValue::from_bits(callback.get_nanbox_f64().to_bits())
                    .as_pointer::<ClosureHeader>()
            };
            assert!(perry_runtime::closure::closure_is_arrow(ptr()));
            assert_eq!(perry_runtime::closure::closure_arity(ptr()), Some(0));
            let name = perry_runtime::closure::closure_get_dynamic_prop(ptr() as usize, "name");
            assert_eq!(string_value_to_string(name).as_deref(), Some(""));
            unsafe {
                perry_runtime::builtins::js_queue_next_tick_args(ptr() as i64, std::ptr::null(), 0);
            }
            (
                callback.get_nanbox_f64().to_bits(),
                object_value(worker.get_raw_mut_ptr()).to_bits(),
            )
        };
        let mut roots = Vec::new();
        perry_runtime::builtins::scan_queued_microtask_roots(&mut |value| {
            roots.push(value.to_bits())
        });
        assert!(roots.contains(&callback_bits));
        let ptr = JSValue::from_bits(callback_bits).as_pointer::<ClosureHeader>();
        assert_eq!(
            perry_runtime::closure::js_closure_get_capture_f64(ptr, 0).to_bits(),
            worker_bits
        );
        let foreign = std::thread::spawn(|| {
            enter_worker_agent();
            perry_runtime::gc::gc_init();
            let mut roots = Vec::new();
            perry_runtime::builtins::scan_queued_microtask_roots(&mut |value| {
                roots.push(value.to_bits())
            });
            roots
        })
        .join()
        .unwrap();
        assert!(!foreign.contains(&callback_bits));
        assert!(!foreign.contains(&worker_bits));
    })
    .join()
    .unwrap();
}

#[test]
fn termination_requests_all_resolve_with_the_observed_exit_code() {
    std::thread::spawn(|| {
        enter_worker_agent();
        perry_runtime::gc::gc_init();
        let scope = RuntimeHandleScope::new();
        let object = scope.root_raw_mut_ptr(perry_runtime::object::js_object_alloc(0, 0));
        let worker = thread_exit_probe::insert_worker_for_test(
            object_value(object.get_raw_mut_ptr()).to_bits(),
            JSValue::undefined().bits(),
        );
        let first = scope.root_nanbox_f64(worker_terminate_by_id(worker));
        let second = scope.root_nanbox_f64(worker_terminate_by_id(worker));
        assert_ne!(
            first.get_nanbox_f64().to_bits(),
            second.get_nanbox_f64().to_bits()
        );
        push_parent_event(
            perry_runtime::agent::current_agent(),
            WorkerEvent::Exit(worker, 0),
        );
        js_worker_threads_process_pending();
        async_shim::js_stdlib_process_pending();
        for promise in [&first, &second] {
            let ptr = JSValue::from_bits(promise.get_nanbox_f64().to_bits())
                .as_pointer::<perry_runtime::Promise>()
                .cast_mut();
            assert_eq!(perry_runtime::js_promise_state(ptr), 1);
            assert_eq!(perry_runtime::js_promise_value(ptr), 0.0);
        }
        WORKERS.lock().unwrap().remove(&worker);
    })
    .join()
    .unwrap();
}

#[test]
fn terminating_an_exited_worker_resolves_undefined() {
    std::thread::spawn(|| {
        enter_worker_agent();
        perry_runtime::gc::gc_init();
        let scope = RuntimeHandleScope::new();
        let object = scope.root_raw_mut_ptr(perry_runtime::object::js_object_alloc(0, 0));
        let worker = thread_exit_probe::insert_worker_for_test(
            object_value(object.get_raw_mut_ptr()).to_bits(),
            JSValue::undefined().bits(),
        );
        WORKERS
            .lock()
            .unwrap()
            .get_mut(&worker)
            .unwrap()
            .set_liveness(false, false);
        let value = worker_terminate_by_id(worker);
        let promise = JSValue::from_bits(value.to_bits())
            .as_pointer::<perry_runtime::Promise>()
            .cast_mut();
        assert_eq!(perry_runtime::js_promise_state(promise), 1);
        assert_eq!(
            perry_runtime::js_promise_value(promise).to_bits(),
            JSValue::undefined().bits()
        );
        WORKERS.lock().unwrap().remove(&worker);
    })
    .join()
    .unwrap();
}
