use super::*;
use crate::component::guest::endpoints::tests::Registry;
use crate::component::guest::frame::*;
use std::cell::RefCell;

crate::perry_thread_local! {
    static WRITES: RefCell<Vec<(*mut c_void, u32, u32, u32)>> = const { RefCell::new(Vec::new()) };
    static NOTICES: RefCell<Vec<(u32, u32, u32, u32)>> = const { RefCell::new(Vec::new()) };
}
unsafe extern "C" fn create(frame: *mut c_void, ty: u32) -> u32 {
    perry_component_frame_endpoint_pair(frame, ty, 0, 1)
}
unsafe extern "C" fn write(
    frame: *mut c_void,
    ty: u32,
    writer: u32,
    operation: u32,
    _: u32,
) -> i32 {
    assert_eq!(io::perry_component_frame_endpoint_write_commit(frame), 0);
    WRITES.with(|calls| calls.borrow_mut().push((frame, ty, writer, operation)));
    0
}
unsafe extern "C" fn notify(ty: u32, handle: u32, operation: u32, action: u32) {
    NOTICES.with(|calls| calls.borrow_mut().push((ty, handle, operation, action)));
}
static TEST_OPS: ComponentGuestProducerOps = ComponentGuestProducerOps {
    abi_version: GUEST_PRODUCER_ABI_VERSION,
    size: std::mem::size_of::<ComponentGuestProducerOps>(),
    create,
    write,
    notify,
};
struct Ops(*mut ComponentGuestProducerOps);
impl Ops {
    fn new() -> Self {
        WRITES.with(|calls| assert!(calls.borrow().is_empty()));
        NOTICES.with(|calls| calls.borrow_mut().clear());
        Self(OPS.swap((&raw const TEST_OPS).cast_mut(), Ordering::AcqRel))
    }
}
impl Drop for Ops {
    fn drop(&mut self) {
        WRITES.with(|calls| {
            for (frame, _, _, _) in std::mem::take(&mut *calls.borrow_mut()) {
                unsafe { perry_component_frame_drop(frame) };
            }
        });
        OPS.store(self.0, Ordering::Release);
    }
}
fn take() -> (*mut c_void, u32, u32, u32) {
    WRITES.with(|calls| {
        assert_eq!(calls.borrow().len(), 1);
        calls.borrow_mut().pop().unwrap()
    })
}
fn factory(key: &str) -> f64 {
    js_component_guest_create_async_value(crate::component::producer::managed_text(key.as_bytes()))
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
fn guest_factory_uses_private_canonical_ends_and_writers_cannot_be_transferred_as_readers() {
    crate::gc::gc_init();
    let _ops = Ops::new();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let pair = scope.root_nanbox_f64(factory("t0"));
    let reader = scope.root_nanbox_f64(field(pair.get_nanbox_f64(), b"value"));
    let writer = scope.root_nanbox_f64(field(pair.get_nanbox_f64(), b"writer"));
    assert_eq!(string(field(pair.get_nanbox_f64(), b"kind")), "future");
    assert_eq!(get(data(reader.get_nanbox_f64()), HANDLE), 0.0);
    assert_eq!(get(writer::data(writer.get_nanbox_f64()), HANDLE), 1.0);
    assert!(JSValue::from_bits(field(writer.get_nanbox_f64(), b"handle").to_bits()).is_undefined());
    assert!(crate::exception::catch_js_throw(|| io::write(writer.get_nanbox_f64(), 42.0)).is_err());
    for key in ["t999", "e0", "bad", "t4294967295"] {
        assert!(crate::exception::catch_js_throw(|| factory(key)).is_err());
    }
    unsafe {
        let frame = perry_component_frame_new();
        let slot = crate::component::guest::frame::frame(frame).add(writer.get_nanbox_f64());
        perry_component_frame_endpoint_lower(frame, slot, 0);
        assert_ne!(perry_component_frame_failed(frame), 0);
        perry_component_frame_drop(frame);
    }
    assert_eq!(get(writer::data(writer.get_nanbox_f64()), STATE), OPEN);
    WRITES.with(|calls| assert!(calls.borrow().is_empty()));
}

#[test]
fn guest_observed_writer_moves_and_releases_temporary_capability_after_settlement() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let _ops = Ops::new();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let domain = crate::async_context::job_domain::JobDomain::new();
    domain.run(|| unsafe {
        let reader = scope.root_nanbox_f64(undefined());
        let weak = scope.root_nanbox_f64(undefined());
        let before;
        {
            let temporary = RuntimeHandleScope::new();
            let pair = temporary.root_nanbox_f64(factory("t0"));
            reader.set_nanbox_f64(field(pair.get_nanbox_f64(), b"value"));
            let writer = temporary.root_nanbox_f64(field(pair.get_nanbox_f64(), b"writer"));
            before = writer.get_nanbox_f64().to_bits();
            weak.set_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::weakref::js_weakref_new(writer.get_nanbox_f64()) as i64,
            ));
            let result = temporary.root_nanbox_f64(io::write(writer.get_nanbox_f64(), 42.0));
            let callback = temporary.root_raw_mut_ptr(crate::closure::js_closure_alloc(
                crate::fn_info!(observed, 1),
                0,
            ));
            crate::promise::then::js_promise_attach_handlers(
                promise(result.get_nanbox_f64()),
                callback.get_raw_const_ptr(),
                std::ptr::null(),
            );
        }
        let (frame, ty, writer_handle, _) = take();
        assert_eq!((ty, writer_handle), (0, 1));
        crate::gc::gc_collect_minor();
        let moved = crate::weakref::js_weakref_deref(weak.get_nanbox_f64());
        assert!(!JSValue::from_bits(moved.to_bits()).is_undefined());
        assert_ne!(moved.to_bits(), before, "the writer must actually relocate");
        assert_eq!(io::perry_component_frame_endpoint_status(frame), 1);
        assert_eq!(io::perry_component_frame_endpoint_prepare(frame), 1);
        let result = perry_component_frame_object(frame);
        assert_eq!(
            io::perry_component_frame_endpoint_finish(frame, result, 1),
            0
        );
        perry_component_frame_drop(frame);
        crate::promise::microtasks::guest_task_checkpoint();
        crate::gc::gc_collect_minor();
        assert!(JSValue::from_bits(
            crate::weakref::js_weakref_deref(weak.get_nanbox_f64()).to_bits()
        )
        .is_undefined());
        NOTICES.with(|notices| assert_eq!(*notices.borrow(), [(0, 1, 0, 1)]));
    });
}

#[test]
fn guest_write_commit_drops_input_expando_cycles_and_old_retirement_cannot_clear_a_retry() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let _ops = Ops::new();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    unsafe {
        (*context::get(super::super::super::registry())).world["types"]["t0"]["ty"]["value"] =
            serde_json::json!({"kind":"record","value":[{"name":"value","ty":{"kind":"u32"}}]});
    }
    let pair = scope.root_nanbox_f64(factory("t0"));
    let writer = scope.root_nanbox_f64(field(pair.get_nanbox_f64(), b"writer"));
    let domain = crate::async_context::job_domain::JobDomain::new();
    domain.run(|| unsafe {
        {
            let temporary = RuntimeHandleScope::new();
            let input = temporary.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::object::js_object_alloc(0, 0) as i64,
            ));
            set_field(input.get_nanbox_f64(), b"value", 42.0);
            let promise = temporary
                .root_nanbox_f64(io::write(writer.get_nanbox_f64(), input.get_nanbox_f64()));
            set_field(
                input.get_nanbox_f64(),
                b"unrelated",
                promise.get_nanbox_f64(),
            );
        }
        let (first, _, _, first_id) = take();
        crate::gc::gc_collect_minor();
        assert_eq!(
            io::perry_component_frame_endpoint_status(first),
            0,
            "C custody must not retain arbitrary input backedges to the Promise"
        );
        assert_eq!(io::perry_component_frame_endpoint_finish(first, 0, 2), 0);
        let next_input = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        set_field(next_input.get_nanbox_f64(), b"value", 99.0);
        let result = scope.root_nanbox_f64(io::write(
            writer.get_nanbox_f64(),
            next_input.get_nanbox_f64(),
        ));
        let (second, _, _, second_id) = take();
        assert_ne!(first_id, second_id);
        perry_component_frame_drop(first);
        assert_eq!(io::perry_component_frame_endpoint_status(second), 1);
        let cancel = field(writer.get_nanbox_f64(), b"cancel");
        crate::closure::js_native_call_value(
            cancel,
            crate::closure::plain_call_receiver(),
            std::ptr::null(),
            0,
        );
        NOTICES.with(|notices| assert_eq!(*notices.borrow(), [(0, 1, second_id, 0)]));
        crate::promise::js_promise_mark_internally_handled(promise(result.get_nanbox_f64()));
        assert_eq!(io::perry_component_frame_endpoint_finish(second, 0, 2), 0);
        perry_component_frame_drop(second);
    });
}
