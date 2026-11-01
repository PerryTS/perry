use super::*;
use crate::component::guest::endpoints::tests::Registry;
use crate::component::guest::frame::*;
use std::cell::RefCell;

crate::perry_thread_local! {
    static STARTED: RefCell<Vec<(*mut c_void, u32, u32, u32)>> = const { RefCell::new(Vec::new()) };
}
unsafe extern "C" fn start(
    frame: *mut c_void,
    ty: u32,
    handle: u32,
    op: u32,
    capacity: u32,
) -> i32 {
    assert_eq!(op, 0);
    STARTED.with(|calls| calls.borrow_mut().push((frame, ty, handle, capacity)));
    0
}
static TEST_OPS: ComponentGuestEndpointOps = ComponentGuestEndpointOps {
    abi_version: GUEST_ENDPOINT_ABI_VERSION,
    size: std::mem::size_of::<ComponentGuestEndpointOps>(),
    operation: start,
};
struct Ops(*mut ComponentGuestEndpointOps);
impl Ops {
    fn new() -> Self {
        STARTED.with(|calls| assert!(calls.borrow().is_empty()));
        Self(OPS.swap((&raw const TEST_OPS).cast_mut(), Ordering::AcqRel))
    }
}
impl Drop for Ops {
    fn drop(&mut self) {
        let pending = STARTED.with(|calls| std::mem::take(&mut *calls.borrow_mut()));
        for (frame, _, _, _) in pending {
            unsafe { perry_component_frame_drop(frame) };
        }
        OPS.store(self.0, Ordering::Release);
    }
}
fn take() -> (*mut c_void, u32, u32, u32) {
    STARTED.with(|calls| {
        assert_eq!(calls.borrow().len(), 1);
        calls.borrow_mut().pop().unwrap()
    })
}
fn promise(value: f64) -> *mut crate::promise::Promise {
    assert_ne!(crate::promise::js_value_is_promise(value), 0);
    JSValue::from_bits(value.to_bits())
        .as_pointer::<crate::promise::Promise>()
        .cast_mut()
}
extern "C" fn observed(
    _: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    _: f64,
) -> f64 {
    undefined()
}

#[test]
fn guest_read_retains_observed_temporaries_through_real_relocation_and_releases_on_settlement() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let _ops = Ops::new();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let domain = job_domain::JobDomain::new();
    domain.run(|| {
        let weak = scope.root_nanbox_f64(undefined());
        let before;
        {
            let temporary = RuntimeHandleScope::new();
            let wrapper = temporary.root_nanbox_f64(wrap(0, 0));
            before = wrapper.get_nanbox_f64().to_bits();
            weak.set_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::weakref::js_weakref_new(wrapper.get_nanbox_f64()) as i64,
            ));
            let result = temporary.root_nanbox_f64(begin(wrapper.get_nanbox_f64(), undefined()));
            let handler = temporary.root_raw_mut_ptr(crate::closure::js_closure_alloc(
                crate::fn_info!(observed, 1),
                0,
            ));
            handler.with_const_ptr(|handler| {
                crate::promise::then::js_promise_attach_handlers(
                    promise(result.get_nanbox_f64()),
                    handler,
                    std::ptr::null(),
                );
            });
        }
        let (frame, ty, handle, capacity) = take();
        assert_eq!((ty, handle, capacity), (0, 0, 1));
        crate::gc::gc_collect_minor();
        let moved = crate::weakref::js_weakref_deref(weak.get_nanbox_f64());
        assert_ne!(
            moved.to_bits(),
            before,
            "the retained capability must actually move"
        );
        assert!(!JSValue::from_bits(moved.to_bits()).is_undefined());
        unsafe {
            assert_eq!(perry_component_frame_endpoint_status(frame), 1);
            assert_eq!(perry_component_frame_endpoint_prepare(frame), 1);
            let result = perry_component_frame_number(frame, 42.0);
            assert_eq!(perry_component_frame_endpoint_finish(frame, result, 1), 0);
            perry_component_frame_drop(frame);
        }
        crate::promise::microtasks::guest_task_checkpoint();
        crate::gc::gc_collect_minor();
        assert!(JSValue::from_bits(
            crate::weakref::js_weakref_deref(weak.get_nanbox_f64()).to_bits()
        )
        .is_undefined());
        assert_eq!(
            unsafe {
                (*context::get(crate::component::guest::registry()))
                    .pending_guest_endpoints
                    .pop_front()
            },
            Some((0, 0))
        );
    });
}

#[test]
fn guest_unobserved_read_is_collectible_and_cancel_ack_reopens_only_its_own_nonce() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let _ops = Ops::new();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let domain = job_domain::JobDomain::new();
    let wrapper = scope.root_nanbox_f64(wrap(2, 17));
    domain.run(|| {
        begin(wrapper.get_nanbox_f64(), 3.0);
        let (first, _, _, capacity) = take();
        assert_eq!(capacity, 3);
        assert!(crate::exception::catch_js_throw(|| begin(wrapper.get_nanbox_f64(), 1.0)).is_err());
        unsafe {
            let transfer = perry_component_frame_new();
            let slot =
                crate::component::guest::frame::frame(transfer).add(wrapper.get_nanbox_f64());
            perry_component_frame_endpoint_lower(transfer, slot, 2);
            assert_ne!(perry_component_frame_failed(transfer), 0);
            perry_component_frame_drop(transfer);
        }
        crate::gc::gc_collect_minor();
        unsafe {
            assert_eq!(
                perry_component_frame_endpoint_status(first),
                0,
                "C must not strongly own an unobserved Promise"
            );
            assert_eq!(perry_component_frame_endpoint_prepare(first), 0);
            assert_eq!(perry_component_frame_endpoint_finish(first, 0, 2), 0);
        }
        let result = scope.root_nanbox_f64(begin(wrapper.get_nanbox_f64(), 1.0));
        let (second, _, _, _) = take();
        unsafe {
            perry_component_frame_drop(first);
            assert_eq!(
                perry_component_frame_endpoint_status(second),
                1,
                "old-frame retirement must not clear the next operation"
            );
            perry_component_frame_drop(second);
        }
        assert_eq!(
            unsafe { (*promise(result.get_nanbox_f64())).state },
            crate::promise::PromiseState::Pending
        );
    });
}

#[test]
fn guest_disposal_rejects_a_live_read_and_terminal_chunks_keep_sticky_eof() {
    crate::gc::gc_init();
    let _ops = Ops::new();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let domain = job_domain::JobDomain::new();
    domain.run(|| unsafe {
        let wrapper = scope.root_nanbox_f64(wrap(2, 71));
        let result = scope.root_nanbox_f64(begin(wrapper.get_nanbox_f64(), 2.0));
        crate::promise::js_promise_mark_internally_handled(promise(result.get_nanbox_f64()));
        let (frame, _, _, _) = take();
        release(data(wrapper.get_nanbox_f64()));
        assert_eq!(perry_component_frame_endpoint_status(frame), 0);
        assert_eq!(perry_component_frame_endpoint_finish(frame, 0, 2), 0);
        assert_eq!(
            (*promise(result.get_nanbox_f64())).state,
            crate::promise::PromiseState::Rejected
        );
        perry_component_frame_drop(frame);
        let wrapper = scope.root_nanbox_f64(wrap(2, 72));
        let result = scope.root_nanbox_f64(begin(wrapper.get_nanbox_f64(), 2.0));
        let (frame, _, _, _) = take();
        assert_eq!(perry_component_frame_endpoint_prepare(frame), 1);
        let chunk = perry_component_frame_object(frame);
        let count = perry_component_frame_number(frame, 17.0);
        perry_component_frame_set(frame, chunk, b"value".as_ptr(), 5, count);
        assert_eq!(perry_component_frame_endpoint_finish(frame, chunk, 1), 0);
        perry_component_frame_drop(frame);
        assert_eq!(
            field((*promise(result.get_nanbox_f64())).value, b"value"),
            17.0
        );
        let eof = scope.root_nanbox_f64(begin(wrapper.get_nanbox_f64(), undefined()));
        assert_eq!(
            field((*promise(eof.get_nanbox_f64())).value, b"done").to_bits(),
            JSValue::bool(true).bits()
        );
        STARTED.with(|calls| assert!(calls.borrow().is_empty()));
    });
}

#[test]
fn guest_read_rejects_out_of_task_bad_capacity_and_foreign_completion() {
    crate::gc::gc_init();
    let _ops = Ops::new();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let wrapper = scope.root_nanbox_f64(wrap(0, 12));
    assert!(
        crate::exception::catch_js_throw(|| begin(wrapper.get_nanbox_f64(), undefined())).is_err()
    );
    let domain = job_domain::JobDomain::new();
    domain.run(|| {
        for capacity in [0.0, -1.0, 1.5, 2.0, f64::NAN, 65537.0] {
            assert!(
                crate::exception::catch_js_throw(|| begin(wrapper.get_nanbox_f64(), capacity))
                    .is_err()
            );
        }
        scope.root_nanbox_f64(begin(wrapper.get_nanbox_f64(), 1.0));
    });
    let (frame, _, _, _) = take();
    let other = job_domain::JobDomain::new();
    other.run(|| unsafe {
        assert_eq!(perry_component_frame_endpoint_prepare(frame), 1);
        let result = perry_component_frame_number(frame, 42.0);
        assert_eq!(perry_component_frame_endpoint_finish(frame, result, 1), -1);
        perry_component_frame_drop(frame);
    });
    assert_eq!(get(data(wrapper.get_nanbox_f64()), PENDING), 0.0);
}

extern "C" fn dispose_in_init(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    _: f64,
    kind: f64,
    _: f64,
    _: f64,
) -> f64 {
    if string(kind) == "PROMISE" {
        release(data(crate::closure::js_closure_get_capture_f64(closure, 0)));
        crate::gc::gc_collect_minor();
    }
    undefined()
}

fn hook(scope: &RuntimeHandleScope, phase: &[u8], callback: f64) -> f64 {
    let callback = scope.root_nanbox_f64(callback);
    let options = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    set_field(options.get_nanbox_f64(), phase, callback.get_nanbox_f64());
    crate::async_hooks::js_async_hooks_create_hook(options.get_nanbox_f64())
}
fn enable(hook: f64) {
    crate::async_hooks::js_async_hook_enable(crate::value::js_nanbox_get_pointer(hook));
}
fn disable(hook: f64) {
    crate::async_hooks::js_async_hook_disable(crate::value::js_nanbox_get_pointer(hook));
}

#[test]
fn guest_read_rechecks_disposal_after_a_collecting_promise_init_hook_before_native_adoption() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let _ops = Ops::new();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let wrapper = scope.root_nanbox_f64(wrap(0, 17));
    let before = wrapper.get_nanbox_f64().to_bits();
    let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(dispose_in_init, 4),
        1,
    ));
    let hook = scope.root_nanbox_f64(callback.with_mut_ptr(|callback| {
        crate::closure::js_closure_set_capture_f64(callback, 0, wrapper.get_nanbox_f64());
        hook(
            &scope,
            b"init",
            crate::value::js_nanbox_pointer(callback as i64),
        )
    }));
    enable(hook.get_nanbox_f64());
    let domain = job_domain::JobDomain::new();
    let outcome = domain
        .run(|| crate::exception::catch_js_throw(|| begin(wrapper.get_nanbox_f64(), undefined())));
    disable(hook.get_nanbox_f64());
    assert!(
        outcome.is_err(),
        "init-hook disposal must reject before C adopts the frame"
    );
    assert_ne!(
        wrapper.get_nanbox_f64().to_bits(),
        before,
        "the hook must actually relocate its captured capability"
    );
    assert_eq!(get(data(wrapper.get_nanbox_f64()), STATE), CLOSED);
    assert_eq!(get(data(wrapper.get_nanbox_f64()), PENDING), 0.0);
    STARTED.with(|calls| assert!(calls.borrow().is_empty()));
}

extern "C" fn change_als_in_init(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    _: f64,
    kind: f64,
    _: f64,
    _: f64,
) -> f64 {
    if string(kind) == "PROMISE" {
        let token = crate::closure::js_closure_get_capture_f64(closure, 0) as i64;
        als::enter_with(token, 22.0);
        crate::gc::gc_collect_minor();
    }
    undefined()
}
extern "C" fn record_als(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    _: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let token = crate::closure::js_closure_get_capture_f64(closure, 0) as i64;
    let log = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 1));
    let index = crate::closure::js_closure_get_capture_f64(closure, 2) as u32;
    crate::gc::gc_collect_minor();
    set(
        log.get_nanbox_f64(),
        index,
        als::get_store(token).expect("read callback ALS"),
    );
    undefined()
}

#[test]
fn guest_read_completion_and_cancel_restore_creation_als_and_preserve_a_sibling_observer() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let _ops = Ops::new();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let mut previous = als::capture_context();
    let previous_roots = als::root_snapshot(&scope, &previous);
    let storage = als::AsyncLocalStoragePayload::default();
    for cancelled in [false, true] {
        let origin = job_domain::JobDomain::new();
        let sibling = job_domain::JobDomain::new();
        als::enter_with(storage.token(), 11.0);
        let wrapper = scope.root_nanbox_f64(wrap(0, 41));
        let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
            crate::fn_info!(change_als_in_init, 4),
            1,
        ));
        let init = scope.root_nanbox_f64(callback.with_mut_ptr(|callback| {
            crate::closure::js_closure_set_capture_f64(callback, 0, storage.token() as f64);
            hook(
                &scope,
                b"init",
                crate::value::js_nanbox_pointer(callback as i64),
            )
        }));
        enable(init.get_nanbox_f64());
        let result =
            scope.root_nanbox_f64(origin.run(|| begin(wrapper.get_nanbox_f64(), undefined())));
        disable(init.get_nanbox_f64());
        assert_eq!(als::get_store(storage.token()), Some(22.0));
        let log = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::array::js_array_alloc_with_length(2) as i64,
        ));
        let mut callbacks = Vec::new();
        for index in 0..2 {
            let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
                crate::fn_info!(record_als, 1),
                3,
            ));
            callbacks.push(scope.root_nanbox_f64(callback.with_mut_ptr(|callback| {
                for (capture, value) in [storage.token() as f64, log.get_nanbox_f64(), index as f64]
                    .into_iter()
                    .enumerate()
                {
                    crate::closure::js_closure_set_capture_f64(callback, capture as u32, value);
                }
                crate::value::js_nanbox_pointer(callback as i64)
            })));
        }
        als::enter_with(storage.token(), 44.0);
        sibling.run(|| {
            let callback = JSValue::from_bits(callbacks[1].get_nanbox_f64().to_bits())
                .as_pointer::<crate::closure::ClosureHeader>();
            crate::promise::then::js_promise_attach_handlers(
                promise(result.get_nanbox_f64()),
                callback,
                callback,
            );
        });
        let resolve = scope.root_nanbox_f64(hook(
            &scope,
            b"promiseResolve",
            callbacks[0].get_nanbox_f64(),
        ));
        enable(resolve.get_nanbox_f64());
        als::enter_with(storage.token(), 33.0);
        let depth = als::context_guard_depth();
        let (frame, _, _, _) = take();
        if cancelled {
            origin.cancel();
            sibling.run(|| unsafe {
                assert_eq!(perry_component_frame_endpoint_status(frame), 0);
                assert_eq!(perry_component_frame_endpoint_finish(frame, 0, 2), 0);
            });
        } else {
            origin.run(|| unsafe {
                assert_eq!(perry_component_frame_endpoint_prepare(frame), 1);
                let value = perry_component_frame_number(frame, 42.0);
                assert_eq!(perry_component_frame_endpoint_finish(frame, value, 1), 0);
            });
        }
        disable(resolve.get_nanbox_f64());
        unsafe { perry_component_frame_drop(frame) };
        assert_eq!(
            get(log.get_nanbox_f64(), 0),
            11.0,
            "settlement hooks use context captured before Promise init hooks"
        );
        assert_eq!(als::get_store(storage.token()), Some(33.0));
        assert_eq!(als::context_guard_depth(), depth);
        sibling.run(crate::promise::microtasks::guest_task_checkpoint);
        assert_eq!(
            get(log.get_nanbox_f64(), 1),
            44.0,
            "a sibling observer survives origin cancellation in its own context"
        );
        assert_eq!(als::get_store(storage.token()), Some(33.0));
        assert_eq!(als::context_guard_depth(), depth);
        assert_eq!(
            unsafe { (*promise(result.get_nanbox_f64())).state },
            if cancelled {
                crate::promise::PromiseState::Rejected
            } else {
                crate::promise::PromiseState::Fulfilled
            }
        );
    }
    als::refresh_snapshot_from_roots(&mut previous, &previous_roots);
    als::restore_context(previous);
}
