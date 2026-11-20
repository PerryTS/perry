use super::*;

#[derive(Default)]
struct InitState {
    calls: Vec<(u64, u64)>,
    requests: Vec<u64>,
    reject: bool,
    pending: bool,
}
struct InitFrame {
    state: *mut InitState,
    marker: u64,
}
unsafe extern "C" fn initializing(
    context: u64,
    origin: u64,
    _: *const u8,
    _: usize,
    _: *const u8,
    _: usize,
    args: *const u8,
    args_len: usize,
    out: *mut Buffer,
) -> i32 {
    let frame = &*(context as usize as *const InitFrame);
    let state = &mut *frame.state;
    state.calls.push((origin, frame.marker));
    if state.pending {
        let args: Value = serde_json::from_slice(bytes(args, args_len).unwrap()).unwrap();
        state
            .requests
            .push(args["$asyncRequest"].as_str().unwrap().parse().unwrap());
        *out = Buffer::default();
        return ASYNC_PENDING;
    }
    if state.reject {
        output(out, b"async initialization reached embedding".to_vec());
        ERROR
    } else {
        output(out, b"null".to_vec());
        OK
    }
}

struct Initialized(*mut c_void);
impl Drop for Initialized {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                perry_component_host_drop(self.0);
            }
        }
    }
}
unsafe fn initialize(async_: bool, state: &mut InitState) -> (Initialized, String) {
    let convention = if async_ { "async " } else { "" };
    let source =
        format!("package test:initialization; world plugin {{ import wait: {convention}func(); }}");
    let world = perry_wit::parse(&source, None).unwrap();
    let contract = serde_json::to_vec(&world).unwrap();
    let component = format!(
        r#"(component
      (import "wait" (func $wait {convention}))
      (core func $wait (canon lower (func $wait) {async_option}))
      (core module $code
        (import "host" "wait" (func $wait {result}))
        (func $start {body})
        (start $start))
      (core instance $code (instantiate $code
        (with "host" (instance (export "wait" (func $wait)))))))"#,
        async_option = if async_ { "async" } else { "" },
        result = if async_ { "(result i32)" } else { "" },
        body = if async_ {
            "(drop (call $wait))"
        } else {
            "(call $wait)"
        }
    );
    let wasm = wat::parse_str(&component).unwrap();
    let frame = InitFrame { state, marker: 10 };
    let mut error = Buffer::default();
    let handle = perry_component_driver_new(
        wasm.as_ptr(),
        wasm.len(),
        contract.as_ptr(),
        contract.len(),
        b"{}".as_ptr(),
        2,
        Some(initializing),
        Some(perry_component_host_buffer_drop),
        None,
        &frame as *const InitFrame as usize as u64,
        &mut error,
    );
    let message = String::from_utf8_lossy(bytes(error.data, error.len).unwrap()).into_owned();
    perry_component_host_buffer_drop(error);
    (Initialized(handle), message)
}

#[test]
fn ffi_driver_core_start_defers_async_import_failure_to_the_first_poll() {
    unsafe {
        let mut sync = InitState::default();
        let (sync_handle, error) = initialize(false, &mut sync);
        assert!(
            !sync_handle.0.is_null(),
            "synchronous initialization witness: {error}"
        );
        assert_eq!(sync.calls, vec![(0, 10)], "initialization uses origin zero");
        drop(sync_handle);
        let mut asynchronous = InitState {
            reject: true,
            ..InitState::default()
        };
        let (handle, error) = initialize(true, &mut asynchronous);
        assert!(
            !handle.0.is_null(),
            "async initialization must construct: {error}"
        );
        assert!(
            asynchronous.calls.is_empty(),
            "async initialization reached constructor: {error}"
        );
        let report = poll_initializing(handle.0, &mut asynchronous, 20);
        assert_eq!(
            asynchronous.calls,
            vec![(0, 20)],
            "first poll must execute the initialization import: {report}"
        );
        assert_eq!(
            report["retired"], true,
            "initialization import failure must retire the Store: {report}"
        );
        assert_eq!(report["failedOrigins"], json!(["0"]));
    }
}

unsafe fn poll_initializing(handle: *mut c_void, state: &mut InitState, marker: u64) -> Value {
    let primary = InitFrame { state, marker };
    let cleanup = InitFrame {
        state,
        marker: marker + 1000,
    };
    let mut out = Buffer::default();
    let status = perry_component_driver_poll(
        handle,
        &primary as *const InitFrame as usize as u64,
        &cleanup as *const InitFrame as usize as u64,
        Some(wake),
        1,
        &mut out,
    );
    reply(status, out).unwrap()
}

#[test]
fn ffi_driver_async_core_start_resolves_without_any_export_invocation() {
    unsafe {
        let mut state = InitState {
            pending: true,
            ..InitState::default()
        };
        let (handle, error) = initialize(true, &mut state);
        assert!(!handle.0.is_null(), "{error}");
        assert!(
            state.calls.is_empty(),
            "initial async import must wait for a poll"
        );
        let pending = poll_initializing(handle.0, &mut state, 20);
        assert_eq!(
            state.calls,
            vec![(0, 20)],
            "initialization must use fresh poll context"
        );
        assert_eq!(state.requests.len(), 1);
        assert_eq!(
            pending["active"], true,
            "pending initialization must keep its embedding active: {pending}"
        );
        assert_eq!(pending["retired"], false);
        assert_eq!(pending["completed"], json!([]));
        assert_eq!(pending["retained"], json!([]));
        assert_eq!(pending["failedOrigins"], json!([]));
        let mut out = Buffer::default();
        let status = perry_component_host_async_import_resolve(
            handle.0,
            state.requests[0],
            true,
            b"null".as_ptr(),
            4,
            &mut out,
        );
        reply(status, out).unwrap();
        for marker in [30, 40] {
            let report = poll_initializing(handle.0, &mut state, marker);
            assert_eq!(
                report["active"], false,
                "resolved initialization must become idle: {report}"
            );
            assert_eq!(report["retired"], false);
            assert_eq!(report["completed"], json!([]));
            assert_eq!(report["failedOrigins"], json!([]));
        }
        assert_eq!(
            state.calls,
            vec![(0, 20)],
            "no constructor frame or duplicate import may be replayed"
        );
    }
}
