use super::*;
use serde_json::{json, Value};
use std::sync::atomic::AtomicUsize;

static FREED: AtomicUsize = AtomicUsize::new(0);

#[derive(Default)]
struct State {
    host: *mut c_void,
    calls: usize,
    reject: bool,
    reentry_rejected: bool,
}

unsafe extern "C" fn free_reply(buffer: Buffer) {
    FREED.fetch_add(1, Ordering::Relaxed);
    perry_component_host_buffer_drop(buffer);
}

unsafe extern "C" fn callback(
    context: u64,
    interface: *const u8,
    interface_len: usize,
    function: *const u8,
    function_len: usize,
    args: *const u8,
    args_len: usize,
    reply: *mut Buffer,
) -> i32 {
    let state = &mut *(context as usize as *mut State);
    assert_eq!(
        bytes(interface, interface_len).unwrap(),
        b"perry:test/logging@1.0.0"
    );
    assert_eq!(bytes(function, function_len).unwrap(), b"log");
    assert_eq!(
        serde_json::from_slice::<Value>(bytes(args, args_len).unwrap()).unwrap(),
        json!(["hello"])
    );
    state.calls += 1;
    let mut error = Buffer::default();
    let status = perry_component_host_dispose(state.host, context, &mut error);
    state.reentry_rejected = status == ERROR
        && String::from_utf8_lossy(bytes(error.data, error.len).unwrap()).contains("re-entered");
    perry_component_host_buffer_drop(error);
    if state.reject {
        output(reply, b"expected import failure".to_vec());
        ERROR
    } else {
        output(reply, b"null".to_vec());
        OK
    }
}

unsafe fn call(state: &mut State, name: &str, args: Value) -> (i32, Vec<u8>) {
    let interface = "perry:test/transform@1.0.0";
    let args = serde_json::to_vec(&args).unwrap();
    let mut reply = Buffer::default();
    let status = perry_component_host_call(
        state.host,
        interface.as_ptr(),
        interface.len(),
        name.as_ptr(),
        name.len(),
        args.as_ptr(),
        args.len(),
        state as *mut State as usize as u64,
        &mut reply,
    );
    let value = bytes(reply.data, reply.len).unwrap().to_vec();
    perry_component_host_buffer_drop(reply);
    (status, value)
}

#[test]
fn callback_buffers_errors_reentry_and_disposal_cross_the_c_boundary() {
    unsafe {
        let wasm = wat::parse_str(crate::tests::COMPONENT).unwrap();
        let world =
            serde_json::to_vec(&perry_wit::parse(crate::tests::WORLD, None).unwrap()).unwrap();
        let mut state = State::default();
        let mut error = Buffer::default();
        let before = FREED.load(Ordering::Relaxed);
        state.host = perry_component_host_new(
            wasm.as_ptr(),
            wasm.len(),
            world.as_ptr(),
            world.len(),
            std::ptr::null(),
            0,
            Some(callback),
            Some(free_reply),
            &mut state as *mut State as usize as u64,
            &mut error,
        );
        assert!(
            !state.host.is_null(),
            "{}",
            String::from_utf8_lossy(bytes(error.data, error.len).unwrap())
        );
        let input = json!({"text":"hello","bytes":[0,255]});
        let (status, reply) = call(&mut state, "echo", json!([input.clone()]));
        assert_eq!(status, OK, "{}", String::from_utf8_lossy(&reply));
        assert_eq!(
            serde_json::from_slice::<Value>(&reply).unwrap(),
            json!({"returned":true,"value":input})
        );
        assert!(state.reentry_rejected);
        // A rejected nested call must not leave the bridge busy.
        assert_eq!(call(&mut state, "returned", json!([])).0, OK);
        state.reject = true;
        let (status, reply) = call(&mut state, "echo", json!([input]));
        assert_eq!(status, ERROR);
        assert!(String::from_utf8_lossy(&reply).contains("expected import failure"));
        assert_eq!(state.calls, 2);
        assert_eq!(FREED.load(Ordering::Relaxed) - before, 2);
        for _ in 0..2 {
            assert_eq!(
                perry_component_host_dispose(
                    state.host,
                    &mut state as *mut State as usize as u64,
                    &mut error
                ),
                OK
            );
            perry_component_host_buffer_drop(error);
            error = Buffer::default();
        }
        let (status, reply) = call(&mut state, "returned", json!([]));
        assert_eq!(status, ERROR);
        assert!(String::from_utf8_lossy(&reply).contains("disposed"));
        perry_component_host_drop(state.host);
    }
}

#[test]
fn malformed_input_returns_an_owned_error_buffer() {
    unsafe {
        let mut error = Buffer::default();
        let pointer = perry_component_host_new(
            std::ptr::null(),
            1,
            b"{}".as_ptr(),
            2,
            std::ptr::null(),
            0,
            None,
            None,
            0,
            &mut error,
        );
        assert!(pointer.is_null());
        assert!(error.len > 0);
        perry_component_host_buffer_drop(error);
        perry_component_host_drop(std::ptr::null_mut());
    }
}

unsafe extern "C" fn failing_destructor(
    _: u64,
    _: *const u8,
    _: usize,
    _: *const u8,
    _: usize,
    _: *const u8,
    _: usize,
    reply: *mut Buffer,
) -> i32 {
    output(reply, b"destructor callback failed".to_vec());
    ERROR
}

#[test]
fn destructor_failure_reports_whether_the_store_or_resource_was_released() {
    for individual in [false, true] {
        unsafe {
            let wasm = wat::parse_str(crate::resource_tests::COMPONENT).unwrap();
            let world =
                serde_json::to_vec(&perry_wit::parse(crate::resource_tests::WORLD, None).unwrap())
                    .unwrap();
            let mut reply = Buffer::default();
            let handle = perry_component_host_new(
                wasm.as_ptr(),
                wasm.len(),
                world.as_ptr(),
                world.len(),
                std::ptr::null(),
                0,
                Some(failing_destructor),
                Some(perry_component_host_buffer_drop),
                1,
                &mut reply,
            );
            assert!(!handle.is_null());
            let interface = b"perry:resources/counters@1.0.0";
            let constructor = b"[constructor]counter";
            assert_eq!(
                perry_component_host_call(
                    handle,
                    interface.as_ptr(),
                    interface.len(),
                    constructor.as_ptr(),
                    constructor.len(),
                    b"[42]".as_ptr(),
                    4,
                    1,
                    &mut reply,
                ),
                OK
            );
            let result: Value =
                serde_json::from_slice(bytes(reply.data, reply.len).unwrap()).unwrap();
            let token = result["value"]["$resource"].as_str().unwrap().to_owned();
            perry_component_host_buffer_drop(reply);
            reply = Buffer::default();
            assert_eq!(
                if individual {
                    perry_component_host_resource_drop(
                        handle,
                        token.as_ptr(),
                        token.len(),
                        1,
                        &mut reply,
                    )
                } else {
                    perry_component_host_dispose(handle, 1, &mut reply)
                },
                RELEASED_ERROR
            );
            assert!(
                String::from_utf8_lossy(bytes(reply.data, reply.len).unwrap())
                    .contains("destructor callback failed")
            );
            perry_component_host_buffer_drop(reply);
            if individual {
                reply = Buffer::default();
                assert_eq!(
                    perry_component_host_resource_drop(
                        handle,
                        token.as_ptr(),
                        token.len(),
                        1,
                        &mut reply,
                    ),
                    ERROR,
                    "an already-consumed resource must not be released twice"
                );
                assert!(
                    String::from_utf8_lossy(bytes(reply.data, reply.len).unwrap())
                        .contains("disposed, transferred")
                );
                perry_component_host_buffer_drop(reply);
            } else {
                assert!((*bridge(handle).unwrap().host.get()).live.is_none());
            }
            reply = Buffer::default();
            assert_eq!(perry_component_host_dispose(handle, 1, &mut reply), OK);
            perry_component_host_buffer_drop(reply);
            perry_component_host_drop(handle);
        }
    }
}
