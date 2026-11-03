use super::*;
use serde_json::{json, Value};
use std::sync::atomic::AtomicUsize;

static WAKES: AtomicUsize = AtomicUsize::new(0);
#[derive(Default)]
struct State {
    host: *mut c_void,
    request: u64,
    calls: usize,
    now: u64,
}
unsafe extern "C" fn wake(_: u64) {
    WAKES.fetch_add(1, Ordering::Relaxed);
}
unsafe fn take(out: Buffer) -> Vec<u8> {
    let value = bytes(out.data, out.len).unwrap().to_vec();
    perry_component_host_buffer_drop(out);
    value
}
unsafe extern "C" fn callback(
    token: u64,
    _: *const u8,
    _: usize,
    function: *const u8,
    function_len: usize,
    args: *const u8,
    args_len: usize,
    out: *mut Buffer,
) -> i32 {
    let state = &mut *(token as usize as *mut State);
    state.calls += 1;
    if bytes(function, function_len).unwrap() == b"now" {
        output(
            out,
            serde_json::to_vec(&json!({"$bigint":state.now.to_string()})).unwrap(),
        );
        state.now += 100;
        return OK;
    }
    let args: Value = serde_json::from_slice(bytes(args, args_len).unwrap()).unwrap();
    assert_eq!(args["args"], json!([{"$bigint":"7"}]));
    state.request = args["$asyncRequest"].as_str().unwrap().parse().unwrap();
    let mut error = Buffer::default();
    assert_eq!(
        perry_component_host_call_async_poll(state.host, token, token, Some(wake), 1, &mut error),
        ERROR
    );
    assert!(String::from_utf8_lossy(&take(error)).contains("re-entered"));
    let mut error = Buffer::default();
    assert_eq!(
        perry_component_host_async_import_resolve(
            state.host,
            state.request,
            true,
            b"null".as_ptr(),
            4,
            &mut error
        ),
        ERROR
    );
    assert!(String::from_utf8_lossy(&take(error)).contains("re-enter"));
    output(out, Vec::new());
    ASYNC_PENDING
}
unsafe fn new(state: &mut State) {
    let component = include_str!("../../../../perry-wasi/src/p3/clock.component.wat").replace(
        "wasi:clocks/monotonic-clock@0.3.0",
        "test:async-clock/clock@1.0.0",
    );
    let wasm = wat::parse_str(component).unwrap();
    let world = perry_wit::parse(
        r#"package test:async-clock@1.0.0;
interface clock { now: func() -> u64; wait-for: async func(how-long: u64); }
world plugin { import clock; export run: async func(nanos: u64) -> u64; }"#,
        None,
    )
    .unwrap();
    let world = serde_json::to_vec(&world).unwrap();
    let mut error = Buffer::default();
    state.host = perry_component_host_new(
        wasm.as_ptr(),
        wasm.len(),
        world.as_ptr(),
        world.len(),
        std::ptr::null(),
        0,
        Some(callback),
        Some(perry_component_host_buffer_drop),
        None,
        state as *mut State as usize as u64,
        &mut error,
    );
    assert!(
        !state.host.is_null(),
        "{}",
        String::from_utf8_lossy(&take(error))
    );
}
unsafe fn start(state: &mut State) -> i32 {
    let args = br#"[{"$bigint":"7"}]"#;
    let token = state as *mut State as usize as u64;
    let mut out = Buffer::default();
    let status = perry_component_host_call_async_start(
        state.host,
        b"".as_ptr(),
        0,
        b"run".as_ptr(),
        3,
        args.as_ptr(),
        args.len(),
        token,
        token,
        Some(wake),
        1,
        &mut out,
    );
    assert!(take(out).is_empty());
    status
}
unsafe fn resolve(state: &State, success: bool) -> (i32, Vec<u8>) {
    let mut out = Buffer::default();
    let value = if success {
        b"null".as_slice()
    } else {
        b"expected rejection".as_slice()
    };
    let status = perry_component_host_async_import_resolve(
        state.host,
        state.request,
        success,
        value.as_ptr(),
        value.len(),
        &mut out,
    );
    (status, take(out))
}

#[test]
fn ffi_async_suspend_resume_refreshes_frames_rejects_reentry_and_reuses_store() {
    unsafe {
        let mut state = State::default();
        new(&mut state);
        for _ in 0..2 {
            assert_eq!(start(&mut state), ASYNC_PENDING);
            assert_ne!(state.request, 0);
            let mut fresh = State {
                host: state.host,
                now: state.now,
                ..State::default()
            };
            let token = &mut fresh as *mut State as usize as u64;
            let mut out = Buffer::default();
            assert_eq!(
                perry_component_host_call_async_poll(
                    state.host,
                    token,
                    token,
                    Some(wake),
                    1,
                    &mut out
                ),
                ASYNC_PENDING
            );
            assert!(take(out).is_empty());
            let before = WAKES.load(Ordering::Relaxed);
            assert_eq!(resolve(&state, true).0, OK);
            assert!(
                WAKES.load(Ordering::Relaxed) > before,
                "settling an import must wake the owner"
            );
            assert_eq!(resolve(&state, true).0, ERROR, "request is single use");
            let mut out = Buffer::default();
            assert_eq!(
                perry_component_host_call_async_poll(
                    state.host,
                    token,
                    token,
                    Some(wake),
                    1,
                    &mut out
                ),
                OK
            );
            assert_eq!(
                serde_json::from_slice::<Value>(&take(out)).unwrap(),
                json!({"returned":true,"value":{"$bigint":"100"}})
            );
            assert_eq!(
                fresh.calls, 1,
                "resumed import must use the new callback frame"
            );
            state.now = fresh.now;
        }
        let mut out = Buffer::default();
        assert_eq!(
            perry_component_host_dispose(
                state.host,
                &mut state as *mut State as usize as u64,
                &mut out
            ),
            OK
        );
        take(out);
        perry_component_host_drop(state.host);
    }
}

#[test]
fn ffi_async_cancel_and_rejection_remove_channels_and_retire_store() {
    unsafe {
        for cancel in [true, false] {
            let mut state = State::default();
            new(&mut state);
            assert_eq!(start(&mut state), ASYNC_PENDING);
            let token = &mut state as *mut State as usize as u64;
            let mut out = Buffer::default();
            if cancel {
                assert_eq!(
                    perry_component_host_dispose(state.host, token, &mut out),
                    OK
                );
                take(out);
            } else {
                assert_eq!(resolve(&state, false).0, OK);
                assert_eq!(
                    perry_component_host_call_async_poll(
                        state.host,
                        token,
                        token,
                        Some(wake),
                        1,
                        &mut out
                    ),
                    RELEASED_ERROR
                );
                assert!(String::from_utf8_lossy(&take(out)).contains("expected rejection"));
            }
            assert_eq!(
                resolve(&state, true).0,
                ERROR,
                "retirement must remove the native reply channel"
            );
            assert!((&*bridge(state.host).unwrap().pending.get()).is_none());
            assert!((&*bridge(state.host).unwrap().host.get()).live.is_none());
            perry_component_host_drop(state.host);
        }
    }
}
