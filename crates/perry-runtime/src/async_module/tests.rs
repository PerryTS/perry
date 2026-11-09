use super::*;
use std::sync::Mutex;
static TRACE: Mutex<Vec<u8>> = Mutex::new(Vec::new());

#[repr(C)]
struct Fixture {
    record: AsyncModule,
    deps: Vec<*mut AsyncModule>,
}
extern "C" fn visit(w: i64, parent: i64) {
    unsafe {
        for &dep in &(*(parent as *mut Fixture)).deps {
            js_async_module_visit(w, parent, dep as i64);
        }
    }
}
unsafe fn module(
    body: extern "C" fn() -> f64,
    flags: u64,
    deps: Vec<*mut AsyncModule>,
) -> *mut AsyncModule {
    let fixture = Box::leak(Box::new(Fixture {
        record: std::mem::zeroed(),
        deps,
    }));
    let record = &mut fixture.record as *mut AsyncModule;
    js_async_module_configure(
        record as i64,
        body as usize as i64,
        visit as *const () as usize as i64,
        0,
        flags,
    );
    record
}
extern "C" fn after_await(_c: *const ClosureHeader, _this: JsThis, _v: f64) -> f64 {
    TRACE.lock().unwrap().push(3);
    UNDEFINED
}
callback_info!(AFTER_AWAIT, after_await);
extern "C" fn suspended() -> f64 {
    TRACE.lock().unwrap().push(1);
    let scope = crate::gc::RuntimeHandleScope::new();
    let p = scope.root_nanbox_f64(js_nanbox_pointer(
        promise::js_promise_resolved(UNDEFINED) as i64
    ));
    let callback = js_closure_alloc(&AFTER_AWAIT, 0);
    js_nanbox_pointer(promise::js_promise_then(
        promise_ptr(p.get_nanbox_f64()),
        callback,
        ptr::null(),
    ) as i64)
}
extern "C" fn rejected_body() -> f64 {
    TRACE.lock().unwrap().push(1);
    js_nanbox_pointer(promise::js_promise_rejected(13.0) as i64)
}
extern "C" fn sibling() -> f64 {
    TRACE.lock().unwrap().push(2);
    UNDEFINED
}
extern "C" fn left() -> f64 {
    TRACE.lock().unwrap().push(4);
    UNDEFINED
}
extern "C" fn right() -> f64 {
    TRACE.lock().unwrap().push(5);
    UNDEFINED
}
extern "C" fn entry() -> f64 {
    TRACE.lock().unwrap().push(9);
    UNDEFINED
}
extern "C" fn throws() -> f64 {
    crate::exception::throw_program_value(17.0)
}
fn reset() {
    crate::gc::js_gc_init();
    TRACE.lock().unwrap().clear();
}
fn trace() -> Vec<u8> {
    TRACE.lock().unwrap().clone()
}

#[test]
fn siblings_start_before_async_continuation_and_dependent_body() {
    reset();
    unsafe {
        let m = module(suspended, 3, vec![]);
        let dependent = module(left, 2, vec![m]);
        let n = module(sibling, 0, vec![]);
        let root = module(entry, 2, vec![m, dependent, n]);
        evaluate(root);
        assert_eq!(trace(), [1, 2]);
        assert_eq!((*dependent).pending, 1);
        assert_eq!((*root).pending, 2);
        promise::js_promise_run_microtasks();
        assert_eq!(trace(), [1, 2, 3, 4, 9]);
        assert_eq!((*root).status, EVALUATED);
    }
}

#[test]
fn diamond_gathers_ancestors_in_async_evaluation_order() {
    reset();
    unsafe {
        let shared = module(suspended, 3, vec![]);
        let l = module(left, 2, vec![shared]);
        let n = module(sibling, 0, vec![]);
        let r = module(right, 2, vec![shared, n]);
        let root = module(entry, 2, vec![l, r]);
        evaluate(root);
        assert!((*l).async_order < (*r).async_order);
        assert_eq!(trace(), [1, 2]);
        promise::js_promise_run_microtasks();
        assert_eq!(trace(), [1, 2, 3, 4, 5, 9]);
    }
}

#[test]
fn cycle_back_edge_does_not_wait_for_its_own_root() {
    reset();
    unsafe {
        let a = module(left, 2, vec![]);
        let b = module(suspended, 3, vec![a]);
        (*(a as *mut Fixture)).deps.push(b);
        let n = module(sibling, 0, vec![]);
        let root = module(entry, 2, vec![a, n]);
        evaluate(root);
        assert_eq!((*a).cycle_root, a);
        assert_eq!((*b).cycle_root, a);
        assert_eq!((*b).pending, 0);
        assert_eq!(trace(), [1, 2]);
        promise::js_promise_run_microtasks();
        assert_eq!(trace(), [1, 2, 3, 4, 9]);
    }
}

#[test]
fn rejected_tla_rejects_parents_without_running_their_bodies() {
    reset();
    unsafe {
        let m = module(rejected_body, 3, vec![]);
        let dependent = module(left, 2, vec![m]);
        let n = module(sibling, 0, vec![]);
        let root = module(entry, 2, vec![m, dependent, n]);
        evaluate(root);
        promise::then::js_promise_attach_handlers(
            promise_ptr((*root).capability),
            ptr::null(),
            ptr::null(),
        );
        assert_eq!(trace(), [1, 2]);
        promise::js_promise_run_microtasks();
        assert_eq!(trace(), [1, 2]);
        assert_eq!((*dependent).status, FAILED);
        assert_eq!((*root).status, FAILED);
        assert_eq!((*root).error, 13.0);
    }
}

#[test]
fn a_ready_synchronous_ancestors_throw_suppresses_later_ancestors() {
    reset();
    unsafe {
        let m = module(suspended, 3, vec![]);
        let bad = module(throws, 2, vec![m]);
        let root = module(entry, 2, vec![bad]);
        evaluate(root);
        promise::then::js_promise_attach_handlers(
            promise_ptr((*root).capability),
            ptr::null(),
            ptr::null(),
        );
        promise::js_promise_run_microtasks();
        assert_eq!(trace(), [1, 3]);
        assert_eq!((*root).status, FAILED);
        assert_eq!((*root).error, 17.0);
    }
}

extern "C" fn observe_rejection(c: *const ClosureHeader, _this: JsThis, _v: f64) -> f64 {
    TRACE
        .lock()
        .unwrap()
        .push(js_closure_get_capture_f64(c, 1) as u8);
    UNDEFINED
}
callback_info!(OBSERVE_REJECTION, observe_rejection);

#[test]
fn rejection_settles_independent_parent_capabilities_in_evaluation_order() {
    reset();
    unsafe {
        let m = module(rejected_body, 3, vec![]);
        let first = module(left, 2, vec![m]);
        let second = module(right, 2, vec![m]);
        evaluate(first);
        let one = handler(&OBSERVE_REJECTION, ptr::null_mut(), 4.0);
        promise::then::js_promise_attach_handlers(
            promise_ptr((*first).capability),
            ptr::null(),
            one,
        );
        evaluate(second);
        let two = handler(&OBSERVE_REJECTION, ptr::null_mut(), 5.0);
        promise::then::js_promise_attach_handlers(
            promise_ptr((*second).capability),
            ptr::null(),
            two,
        );
        promise::js_promise_run_microtasks();
        assert_eq!(trace(), [1, 4, 5]);
        assert_eq!((*first).status, FAILED);
        assert_eq!((*second).status, FAILED);
    }
}
