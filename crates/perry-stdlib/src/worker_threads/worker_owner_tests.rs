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
