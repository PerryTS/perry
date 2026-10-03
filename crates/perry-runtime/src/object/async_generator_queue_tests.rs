//! Queue metadata must root pending work without retaining idle generators.
use super::*;
use crate::closure::{js_closure_alloc, js_closure_call1, ClosureHeader, JsThis};
use crate::value::{js_nanbox_pointer, TAG_UNDEFINED};

extern "C" fn pending_step(_: *const ClosureHeader, _: JsThis, _: f64) -> f64 {
    js_nanbox_pointer(crate::promise::js_promise_new() as i64)
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
