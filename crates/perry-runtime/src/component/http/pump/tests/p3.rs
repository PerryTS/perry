use super::*;
use perry_abi::component_host::{WakeCallback, ASYNC_PENDING};
static PRESERVE: AtomicBool = AtomicBool::new(false);
static COMPLETED: AtomicBool = AtomicBool::new(false);
unsafe extern "C" fn start(
    _: *mut c_void,
    request: *mut c_void,
    _: *const u8,
    _: usize,
    _: u64,
    _: u64,
    _: Option<WakeCallback>,
    _: u64,
    out: *mut Buffer,
) -> i32 {
    assert_eq!(
        DROPS.load(Ordering::Acquire),
        0,
        "the handoff retains its native accept owner"
    );
    (*request.cast::<Exchange>()).invoked = true;
    CALLS.fetch_add(1, Ordering::Relaxed);
    *out = Buffer::default();
    ASYNC_PENDING
}
unsafe extern "C" fn poll(
    _: *mut c_void,
    primary: u64,
    _: u64,
    _: Option<WakeCallback>,
    _: u64,
    out: *mut Buffer,
) -> i32 {
    *out = Buffer::default();
    if !COMPLETED.load(Ordering::Acquire) {
        return ASYNC_PENDING;
    }
    crate::gc::gc_collect_minor();
    if FAIL.load(Ordering::Acquire) {
        if PRESERVE.load(Ordering::Acquire) {
            reply(out, b"HTTP request denied");
            return perry_abi::component_host::HTTP_FAILURE;
        }
        let frame = &*(primary as usize as *const callback::Frame<'_>);
        frame.capture(field(
            context::imports(frame.owner.get_nanbox_f64()),
            b"expected",
        ));
        reply(out, b"P3 primary failure");
        RELEASED_ERROR
    } else {
        reply(out, br#"{"returned":false}"#);
        OK
    }
}
static ASYNC_OPS: ComponentHostOps = ComponentHostOps {
    perry_component_http_handle_async_start: start,
    perry_component_host_call_async_poll: poll,
    ..OPS
};
struct Lease(*mut context::Context);
impl Drop for Lease {
    fn drop(&mut self) {
        unsafe { (*self.0).host = std::ptr::null_mut() }
    }
}
#[test]
fn p3_http_handoff_traces_its_moving_accept_owner_and_settles_the_original_promise() {
    for mode in 0..3 {
        let reject = mode != 0;
        let preserve = mode == 2;
        PRESERVE.store(preserve, Ordering::Release);
        let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
        crate::gc::gc_init();
        let _engine = host::TestOpsGuard::install(&ASYNC_OPS);
        reset();
        COMPLETED.store(false, Ordering::Release);
        let scope = RuntimeHandleScope::new();
        let expected = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        let imports = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        set_field(
            imports.get_nanbox_f64(),
            b"expected",
            expected.get_nanbox_f64(),
        );
        let component = scope.root_nanbox_f64(context::publish(
            serde_json::json!({}),
            imports.get_nanbox_f64(),
        ));
        unsafe {
            (*context::get(component.get_nanbox_f64())).host = std::ptr::dangling_mut::<u8>().cast()
        };
        let _lease = Lease(context::get(component.get_nanbox_f64()));
        let options = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        let server = scope.root_nanbox_f64(js_component_http_server_new(options.get_nanbox_f64()));
        let interface = scope.root_string_ptr(crate::string::js_string_from_bytes(
            b"wasi:http/handler@0.3.0".as_ptr(),
            23,
        ));
        let promise = scope.root_nanbox_f64(js_component_http_handle(
            server.get_nanbox_f64(),
            component.get_nanbox_f64(),
            crate::value::js_nanbox_string(
                interface.get_raw_const_ptr::<crate::string::StringHeader>() as i64,
            ),
        ));
        crate::promise::js_promise_mark_internally_handled(
            JSValue::from_bits(promise.get_nanbox_f64().to_bits())
                .as_pointer::<crate::promise::Promise>()
                .cast_mut(),
        );
        let weak = scope.root_nanbox_f64(undefined());
        let original;
        {
            let temporary = RuntimeHandleScope::new();
            let link = PENDING.with(|q| q.borrow()[0]);
            let owner =
                temporary.root_nanbox_f64(unsafe { payload::link_event_owner(link) }.unwrap());
            original = owner.get_nanbox_f64().to_bits();
            weak.set_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::weakref::js_weakref_new(owner.get_nanbox_f64()) as i64,
            ));
        }
        READY.store(true, Ordering::Release);
        assert_eq!(pump(), 1);
        assert_eq!(
            has_active(),
            0,
            "HTTP pump releases its pin after delegation"
        );
        crate::gc::gc_collect_minor();
        let moved = crate::weakref::js_weakref_deref(weak.get_nanbox_f64());
        assert!(!JSValue::from_bits(moved.to_bits()).is_undefined());
        assert_ne!(
            moved.to_bits(),
            original,
            "only the async child keeps the evacuated HTTP owner alive"
        );
        assert_eq!(DROPS.load(Ordering::Acquire), 0);
        crate::stdlib_pump::js_run_agent_pumps();
        assert_eq!(CALLS.load(Ordering::Acquire), 1);
        assert_eq!(
            crate::promise::js_promise_state(
                JSValue::from_bits(promise.get_nanbox_f64().to_bits())
                    .as_pointer::<crate::promise::Promise>()
                    .cast_mut()
            ),
            0
        );
        FAIL.store(reject, Ordering::Release);
        COMPLETED.store(true, Ordering::Release);
        crate::stdlib_pump::js_run_agent_pumps();
        let pointer = JSValue::from_bits(promise.get_nanbox_f64().to_bits())
            .as_pointer::<crate::promise::Promise>()
            .cast_mut();
        assert_eq!(
            crate::promise::js_promise_state(pointer),
            if reject { 2 } else { 1 }
        );
        if reject && !preserve {
            assert_eq!(
                crate::promise::js_promise_result(pointer).to_bits(),
                expected.get_nanbox_f64().to_bits()
            )
        }
        assert_eq!(
            DROPS.load(Ordering::Acquire),
            1,
            "accept released exactly once before observable settlement"
        );
        assert!(!unsafe { (*context::get(component.get_nanbox_f64())).async_active });
        assert_eq!(
            unsafe { (*context::get(component.get_nanbox_f64())).alive },
            !reject || preserve,
            "a completed HTTP error preserves its component"
        );
        js_component_http_server_close(server.get_nanbox_f64());
    }
}
