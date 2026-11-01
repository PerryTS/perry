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
            crate::promise::then::js_promise_attach_handlers(
                promise(result.get_nanbox_f64()),
                handler.get_raw_const_ptr(),
                std::ptr::null(),
            );
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
