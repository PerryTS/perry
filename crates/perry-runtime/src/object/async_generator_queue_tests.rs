//! Queue metadata must root pending work without retaining idle generators.
use super::*;
use crate::closure::{js_closure_alloc, js_closure_call1, ClosureHeader, JsThis};
use crate::value::{js_nanbox_pointer, TAG_UNDEFINED};

extern "C" fn pending_step(_: *const ClosureHeader, _: JsThis, _: f64) -> f64 {
    js_nanbox_pointer(crate::promise::js_promise_new() as i64)
}

/// A queue entry built from rooted handles. Nothing allocates while the
/// scoped pointers are alive, so no pointer outlives the borrow.
fn queued_request(
    original: &crate::gc::RuntimeHandle<'_>,
    original_throw: &crate::gc::RuntimeHandle<'_>,
    arg: f64,
    promise: &crate::gc::RuntimeHandle<'_>,
    kind: RequestKind,
) -> AsyncGeneratorRequest {
    original.with_mut_ptr(|original| {
        original_throw.with_mut_ptr(|original_throw| {
            promise.with_mut_ptr(|promise| AsyncGeneratorRequest {
                original,
                original_throw,
                arg,
                promise,
                kind,
            })
        })
    })
}

fn scanned_roots() -> Vec<u64> {
    let mut roots = Vec::new();
    let mut mark = |value: f64| roots.push(value.to_bits());
    scan_async_generator_queue_roots_mut(&mut crate::gc::RuntimeRootVisitor::for_copy(&mut mark));
    roots
}

#[test]
fn idle_async_generator_is_not_a_queue_root_but_queued_work_is() {
    std::thread::spawn(|| {
        crate::gc::gc_init();
        let scope = crate::gc::RuntimeHandleScope::new();
        let object = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        for name in [
            b"next".as_slice(),
            b"return".as_slice(),
            b"throw".as_slice(),
        ] {
            let closure =
                scope.root_raw_mut_ptr(js_closure_alloc(crate::fn_info!(pending_step, 1), 0));
            object.with_mut_ptr(|object| {
                closure.with_mut_ptr(|closure| {
                    set_method(object, name, closure);
                })
            });
        }
        object.with_mut_ptr(wrap_async_generator_instance);
        assert!(
            scanned_roots().is_empty(),
            "an unstarted instance must not become immortal"
        );
        let next = object.with_mut_ptr(|object| own_closure(object, b"next").unwrap());
        let next = scope.root_raw_const_ptr(next);
        let arg = f64::from_bits(TAG_UNDEFINED);
        next.with_const_ptr(|next| {
            js_closure_call1(next, crate::closure::plain_call_receiver(), arg);
        });
        assert!(
            scanned_roots().is_empty(),
            "an idle suspended instance must not become immortal"
        );
        next.with_const_ptr(|next| {
            js_closure_call1(next, crate::closure::plain_call_receiver(), arg);
        });
        let roots = scanned_roots();
        let original_throw = object.with_mut_ptr(|object| own_closure(object, b"throw").unwrap());
        let original_throw = crate::closure::js_closure_get_capture_ptr(original_throw, 1);
        assert!(
            roots.contains(&crate::value::JSValue::pointer(original_throw as *const u8).bits()),
            "a queued request must keep its throw continuation alive"
        );
        assert!(
            roots.len() >= 3,
            "queued original, throw and result promise must be roots"
        );
    })
    .join()
    .unwrap();
}

extern "C" fn counted_step(closure: *const ClosureHeader, _: JsThis, _: f64) -> f64 {
    let count = js_closure_get_capture_f64(closure, 0);
    js_closure_set_capture_f64(closure as *mut _, 0, count + 1.0);
    boxed_promise(crate::promise::js_promise_resolved(42.0))
}

#[test]
fn pending_step_completes_and_drains_before_the_next_job() {
    for fulfilled in [true, false] {
        std::thread::spawn(move || {
            crate::gc::gc_init();
            let scope = crate::gc::RuntimeHandleScope::new();
            let step =
                scope.root_raw_mut_ptr(js_closure_alloc(crate::fn_info!(counted_step, 1), 1));
            step.with_mut_ptr(|step| js_closure_set_capture_f64(step, 0, 0.0));
            let out = scope.root_raw_mut_ptr(js_promise_new());
            let queued = scope.root_raw_mut_ptr(js_promise_new());
            let pending = scope.root_raw_mut_ptr(js_promise_new());
            let state_id = STATES.with(|states| {
                let mut states = states.borrow_mut();
                states.push(AsyncGeneratorQueueState {
                    active: true,
                    completed: false,
                    queue: VecDeque::from([queued_request(
                        &step,
                        &step,
                        0.0,
                        &queued,
                        RequestKind::NextOrThrow,
                    )]),
                });
                states.len()
            });
            pending.with_mut_ptr(|pending| {
                out.with_mut_ptr(|out| attach_pending_settle(state_id, pending, out))
            });
            pending.with_mut_ptr(|pending| {
                if fulfilled {
                    js_promise_resolve(pending, 7.0);
                } else {
                    js_promise_reject(pending, 7.0);
                }
            });
            assert_eq!(
                step.with_mut_ptr(|step| js_closure_get_capture_f64(step, 0)),
                1.0,
                "queue resumption belongs to settlement, not a later reaction job"
            );
            out.with_mut_ptr(|out: *mut Promise| unsafe {
                assert_eq!(
                    (*out).state,
                    if fulfilled {
                        PromiseState::Fulfilled
                    } else {
                        PromiseState::Rejected
                    }
                );
            });
            queued.with_mut_ptr(|queued: *mut Promise| unsafe {
                assert_eq!((*queued).state, PromiseState::Fulfilled);
            });
            STATES.with(|states| assert!(!states.borrow()[state_id - 1].active));
        })
        .join()
        .unwrap();
    }
}

#[test]
fn immediate_step_drains_a_long_queue_in_the_same_turn() {
    std::thread::spawn(|| {
        crate::gc::gc_init();
        let scope = crate::gc::RuntimeHandleScope::new();
        let step = scope.root_raw_mut_ptr(js_closure_alloc(crate::fn_info!(counted_step, 1), 1));
        step.with_mut_ptr(|step| js_closure_set_capture_f64(step, 0, 0.0));
        let out = scope.root_raw_mut_ptr(js_promise_new());
        let queued = scope.root_raw_mut_ptr(js_promise_new());
        let id = STATES.with(|states| {
            let mut states = states.borrow_mut();
            states.push(AsyncGeneratorQueueState {
                active: true,
                completed: false,
                queue: (0..8192)
                    .map(|_| queued_request(&step, &step, 0.0, &queued, RequestKind::NextOrThrow))
                    .collect(),
            });
            states.len()
        });
        out.with_mut_ptr(|out| finish_step(id, out, true, 7.0));
        assert_eq!(
            step.with_mut_ptr(|step| js_closure_get_capture_f64(step, 0)),
            8192.0
        );
        queued.with_mut_ptr(|queued: *mut Promise| unsafe {
            assert_eq!((*queued).state, PromiseState::Fulfilled);
        });
    })
    .join()
    .unwrap();
}

extern "C" fn observe_front_settlement(closure: *const ClosureHeader, _: JsThis, _: f64) -> f64 {
    let front = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    js_closure_set_capture_f64(
        closure as *mut _,
        1,
        crate::promise::js_promise_state(front) as f64,
    );
    boxed_promise(crate::promise::js_promise_resolved(42.0))
}

#[test]
fn front_request_is_settled_before_resuming_the_next_request() {
    std::thread::spawn(|| {
        crate::gc::gc_init();
        let scope = crate::gc::RuntimeHandleScope::new();
        let front = scope.root_raw_mut_ptr(js_promise_new());
        let queued = scope.root_raw_mut_ptr(js_promise_new());
        let step = scope.root_raw_mut_ptr(js_closure_alloc(
            crate::fn_info!(observe_front_settlement, 1),
            2,
        ));
        step.with_mut_ptr(|step| {
            front.with_mut_ptr::<Promise, _>(|front| {
                js_closure_set_capture_ptr(step, 0, front as i64);
            })
        });
        let id = STATES.with(|states| {
            let mut states = states.borrow_mut();
            states.push(AsyncGeneratorQueueState {
                active: true,
                completed: false,
                queue: VecDeque::from([queued_request(
                    &step,
                    &step,
                    0.0,
                    &queued,
                    RequestKind::NextOrThrow,
                )]),
            });
            states.len()
        });
        front.with_mut_ptr(|front| finish_step(id, front, true, 7.0));
        assert_eq!(
            step.with_mut_ptr(|step| js_closure_get_capture_f64(step, 1)),
            1.0,
            "the next body runs after the front promise is fulfilled"
        );
    })
    .join()
    .unwrap();
}

#[test]
fn rejected_return_resumes_a_suspended_yield_as_throw() {
    for suspended in [true, false] {
        std::thread::spawn(move || {
            crate::gc::gc_init();
            let scope = crate::gc::RuntimeHandleScope::new();
            let step =
                scope.root_raw_mut_ptr(js_closure_alloc(crate::fn_info!(counted_step, 1), 1));
            step.with_mut_ptr(|step| js_closure_set_capture_f64(step, 0, 0.0));
            let out = scope.root_raw_mut_ptr(js_promise_new());
            let id = STATES.with(|states| {
                let mut states = states.borrow_mut();
                states.push(AsyncGeneratorQueueState {
                    active: true,
                    completed: !suspended,
                    queue: VecDeque::new(),
                });
                states.len()
            });
            let wrapper = make_return_step_wrapper(id, &step, &out, false);
            async_generator_return_step_reject(wrapper, crate::closure::plain_call_receiver(), 7.0);
            assert_eq!(
                step.with_mut_ptr(|step| js_closure_get_capture_f64(step, 0)),
                if suspended { 1.0 } else { 0.0 }
            );
            out.with_mut_ptr(|out: *mut Promise| unsafe {
                assert_eq!(
                    (*out).state,
                    if suspended {
                        PromiseState::Fulfilled
                    } else {
                        PromiseState::Rejected
                    }
                );
            });
        })
        .join()
        .unwrap();
    }
}

extern "C" fn close_start_on_throw(closure: *const ClosureHeader, _: JsThis, reason: f64) -> f64 {
    let next = js_closure_get_capture_ptr(closure, 0) as *mut ClosureHeader;
    js_closure_set_capture_f64(next, 0, 1.0);
    boxed_promise(crate::promise::js_promise_rejected(reason))
}

extern "C" fn observe_closed_start(closure: *const ClosureHeader, _: JsThis, _: f64) -> f64 {
    let closed = js_closure_get_capture_f64(closure, 0);
    js_closure_set_capture_f64(closure as *mut _, 1, closed);
    boxed_promise(crate::promise::js_promise_resolved(42.0))
}

#[test]
fn rejected_return_closes_suspended_start_before_draining() {
    std::thread::spawn(|| {
        crate::gc::gc_init();
        let scope = crate::gc::RuntimeHandleScope::new();
        let next = scope.root_raw_mut_ptr(js_closure_alloc(
            crate::fn_info!(observe_closed_start, 1),
            2,
        ));
        let throw = scope.root_raw_mut_ptr(js_closure_alloc(
            crate::fn_info!(close_start_on_throw, 1),
            1,
        ));
        next.with_mut_ptr(|c| {
            js_closure_set_capture_f64(c, 0, 0.0);
            js_closure_set_capture_f64(c, 1, 0.0);
            throw.with_mut_ptr(|throw| {
                crate::closure::js_closure_set_capture_ptr(throw, 0, c as i64)
            });
        });
        let out = scope.root_raw_mut_ptr(js_promise_new());
        let queued = scope.root_raw_mut_ptr(js_promise_new());
        let id = STATES.with(|states| {
            let mut states = states.borrow_mut();
            states.push(AsyncGeneratorQueueState {
                active: true,
                completed: false,
                queue: VecDeque::from([queued_request(
                    &next,
                    &throw,
                    0.0,
                    &queued,
                    RequestKind::NextOrThrow,
                )]),
            });
            states.len()
        });
        let wrapper = make_return_step_wrapper(id, &throw, &out, false);
        async_generator_return_step_reject(wrapper, crate::closure::plain_call_receiver(), 7.0);
        assert_eq!(
            next.with_mut_ptr(|c| js_closure_get_capture_f64(c, 1)),
            1.0,
            "the generator's throw transition must close suspendedStart before next resumes"
        );
        out.with_mut_ptr(|p: *mut Promise| unsafe {
            assert_eq!((*p).state, PromiseState::Rejected);
            assert_eq!((*p).reason, 7.0);
        });
        queued.with_mut_ptr(|p: *mut Promise| unsafe {
            assert_eq!((*p).state, PromiseState::Fulfilled);
        });
    })
    .join()
    .unwrap();
}
