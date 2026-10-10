//! Producer completion is synchronous; consumer reactions remain jobs.
use super::*;
use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64, ClosureHeader, JsThis,
};

extern "C" fn completion(
    closure: *const ClosureHeader,
    _: JsThis,
    value: f64,
    fulfilled: f64,
) -> f64 {
    js_closure_set_capture_f64(closure as *mut _, 0, value);
    js_closure_set_capture_f64(closure as *mut _, 1, fulfilled);
    js_closure_set_capture_f64(closure as *mut _, 2, 1.0);
    value
}
extern "C" fn reaction(closure: *const ClosureHeader, _: JsThis, value: f64) -> f64 {
    js_closure_set_capture_f64(closure as *mut _, 0, value);
    js_closure_set_capture_f64(closure as *mut _, 1, 1.0);
    value
}

#[test]
fn completion_is_once_per_settlement_and_reactions_stay_asynchronous() {
    for fulfilled in [true, false] {
        std::thread::spawn(move || {
            crate::gc::gc_init();
            let scope = crate::gc::RuntimeHandleScope::new();
            let promise = scope.root_raw_mut_ptr(crate::promise::js_promise_new());
            let complete =
                scope.root_raw_mut_ptr(js_closure_alloc(crate::fn_info!(completion, 2), 3));
            let consumer =
                scope.root_raw_mut_ptr(js_closure_alloc(crate::fn_info!(reaction, 1), 2));
            complete.with_mut_ptr(|c| js_closure_set_capture_f64(c, 2, 0.0));
            consumer.with_mut_ptr(|c| js_closure_set_capture_f64(c, 1, 0.0));
            promise.with_mut_ptr(|p| complete.with_mut_ptr(|c| attach_step_completion(p, c)));
            promise.with_mut_ptr(|p| {
                consumer.with_mut_ptr(|c| {
                    crate::promise::js_promise_then(p, c, c);
                })
            });
            promise.with_mut_ptr(|p| {
                if fulfilled {
                    crate::promise::js_promise_resolve(p, 73.0);
                } else {
                    crate::promise::js_promise_reject(p, 73.0);
                }
            });
            complete.with_mut_ptr(|c| {
                assert_eq!(js_closure_get_capture_f64(c, 0), 73.0);
                assert_eq!(
                    crate::value::js_is_truthy(js_closure_get_capture_f64(c, 1)) != 0,
                    fulfilled
                );
                assert_eq!(js_closure_get_capture_f64(c, 2), 1.0);
            });
            consumer.with_mut_ptr(|c| assert_eq!(js_closure_get_capture_f64(c, 1), 0.0));
            crate::promise::js_promise_run_microtasks();
            consumer.with_mut_ptr(|c| {
                assert_eq!(js_closure_get_capture_f64(c, 0), 73.0);
                assert_eq!(js_closure_get_capture_f64(c, 1), 1.0);
            });
            promise.with_mut_ptr(|p| crate::promise::js_promise_resolve(p, 99.0));
            complete.with_mut_ptr(|c| assert_eq!(js_closure_get_capture_f64(c, 0), 73.0));
            promise.with_mut_ptr(|p: *mut Promise| unsafe {
                assert_eq!(
                    if fulfilled { (*p).reason } else { (*p).value }.to_bits(),
                    0,
                    "the completion's inactive result word must be cleared"
                );
            });
        })
        .join()
        .unwrap();
    }
}

extern "C" fn rejected_step(_: *const ClosureHeader, _: JsThis, _: f64, _: f64) -> f64 {
    crate::value::js_nanbox_pointer(crate::promise::js_promise_rejected(73.0) as i64)
}
extern "C" fn observe_step_rejection(closure: *const ClosureHeader, _: JsThis) -> f64 {
    let promise = crate::closure::js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    js_closure_set_capture_f64(
        closure as *mut _,
        1,
        crate::promise::js_promise_state(promise) as f64,
    );
    0.0
}

#[test]
fn direct_step_forwards_an_abrupt_completion_before_the_next_job() {
    std::thread::spawn(|| {
        crate::gc::gc_init();
        let scope = crate::gc::RuntimeHandleScope::new();
        let promise = scope.root_raw_mut_ptr(crate::promise::js_promise_new());
        let step = scope.root_raw_mut_ptr(js_closure_alloc(crate::fn_info!(rejected_step, 2), 0));
        let observer = scope.root_raw_mut_ptr(js_closure_alloc(
            crate::fn_info!(observe_step_rejection, 0),
            2,
        ));
        promise.with_mut_ptr(|p| {
            crate::promise::js_promise_mark_internally_handled(p);
            observer.with_mut_ptr(|c| crate::closure::js_closure_set_capture_ptr(c, 0, p as i64));
        });
        let context = crate::async_context::capture_context();
        step.with_mut_ptr(|step| {
            promise.with_mut_ptr(|promise| {
                crate::promise::TASK_QUEUE.with(|queue| {
                    queue
                        .borrow_mut()
                        .push_back(crate::promise::Task::AsyncStep(
                            step,
                            0.0,
                            promise,
                            false,
                            context,
                            std::ptr::null_mut(),
                            0,
                            0,
                        ));
                });
            })
        });
        observer.with_mut_ptr(|c: *mut ClosureHeader| {
            crate::promise::enqueue_queue_microtask(c as i64)
        });
        crate::promise::js_promise_run_microtasks();
        assert_eq!(
            observer.with_mut_ptr(|c| js_closure_get_capture_f64(c, 1)),
            2.0,
            "step rejection must settle before a later queued job"
        );
    })
    .join()
    .unwrap();
}
