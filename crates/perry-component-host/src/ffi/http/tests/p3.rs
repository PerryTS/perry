use super::*;
use crate::ffi::{
    perry_component_host_async_import_resolve, perry_component_host_call_async_poll,
    perry_component_host_dispose,
};
use perry_abi::component_host::ASYNC_PENDING;

const P3: &str = "wasi:http/handler@0.3.0";
const WAT: &str = include_str!("../../../http/p3/bridge.component.wat");
const WORLD: &str = include_str!("../../../../../../test-wasi/components/http-p3/world.wit");
static REQUEST: Mutex<Option<(u64, u64)>> = Mutex::new(None);
unsafe extern "C" fn before(
    context: u64,
    _: *const u8,
    _: usize,
    name: *const u8,
    len: usize,
    args: *const u8,
    args_len: usize,
    out: *mut Buffer,
) -> i32 {
    assert_eq!(std::slice::from_raw_parts(name, len), b"before");
    let args: serde_json::Value =
        serde_json::from_slice(std::slice::from_raw_parts(args, args_len)).unwrap();
    *REQUEST.lock().unwrap() = Some((
        args["$asyncRequest"].as_str().unwrap().parse().unwrap(),
        context,
    ));
    *out = Buffer::default();
    ASYNC_PENDING
}
fn component(source: &str) -> Component {
    *REQUEST.lock().unwrap() = None;
    let wasm = wat::parse_str(source).unwrap();
    let contract = serde_json::to_vec(&perry_wit::parse(WORLD, None).unwrap()).unwrap();
    let options = br#"{"wasi":{}}"#;
    let mut out = Buffer::default();
    let host = unsafe {
        perry_component_host_new(
            wasm.as_ptr(),
            wasm.len(),
            contract.as_ptr(),
            contract.len(),
            options.as_ptr(),
            options.len(),
            Some(before),
            Some(perry_component_host_buffer_drop),
            None,
            1,
            &mut out,
        )
    };
    let bytes = unsafe { take_output(out) };
    assert!(!host.is_null(), "{}", String::from_utf8_lossy(&bytes));
    Component(host)
}
fn start(
    component: &Component,
    op: &Op,
    interface: &str,
    primary: u64,
    cleanup: u64,
) -> (i32, Vec<u8>) {
    let mut out = Buffer::default();
    let status = unsafe {
        perry_component_http_handle_async_start(
            component.0,
            op.0,
            interface.as_ptr(),
            interface.len(),
            primary,
            cleanup,
            Some(notify),
            42,
            &mut out,
        )
    };
    (status, unsafe { take_output(out) })
}
fn poll(component: &Component, primary: u64, cleanup: u64) -> (i32, Vec<u8>) {
    let mut out = Buffer::default();
    let status = unsafe {
        perry_component_host_call_async_poll(
            component.0,
            primary,
            cleanup,
            Some(notify),
            42,
            &mut out,
        )
    };
    (status, unsafe { take_output(out) })
}
fn finish(component: &Component) -> (i32, Vec<u8>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let generation = *WAKE.0.lock().unwrap();
        let result = poll(component, 300, 301);
        if result.0 != ASYNC_PENDING {
            return result;
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .expect("P3 HTTP FFI completion watchdog");
        let (value, timeout) = WAKE
            .1
            .wait_timeout_while(WAKE.0.lock().unwrap(), remaining, |v| *v == generation)
            .unwrap();
        assert!(
            !timeout.timed_out() || *value != generation,
            "P3 HTTP missing wake"
        );
    }
}
fn resolve(component: &Component, request: u64) -> i32 {
    let mut out = Buffer::default();
    let status = unsafe {
        perry_component_host_async_import_resolve(
            component.0,
            request,
            true,
            b"null".as_ptr(),
            4,
            &mut out,
        )
    };
    unsafe { take_output(out) };
    status
}
#[test]
fn p3_http_ffi_transfers_request_resumes_under_fresh_frames_and_reuses_host() {
    let host = component(WAT);
    let server = Listener::new();
    for _ in 0..2 {
        let op = server.accept();
        let peer = server.peer();
        assert_eq!(op.wait().0, HTTP_READY);
        unsafe { bridge(host.0).unwrap().busy.set(true) };
        assert_eq!(start(&host, &op, P3, 100, 101).0, ERROR);
        unsafe { bridge(host.0).unwrap().busy.set(false) };
        assert_eq!(op.poll().0, HTTP_READY);
        assert_eq!(start(&host, &op, P3, 100, 101).0, ASYNC_PENDING);
        // The future owns the native request and completion after the start
        // call returns; its accept handle can now be dropped independently.
        drop(op);
        assert_eq!(poll(&host, 200, 201).0, ASYNC_PENDING);
        let (id, context) = REQUEST
            .lock()
            .unwrap()
            .take()
            .expect("suspended async import");
        assert!(context == 100 || context == 200);
        assert_eq!(resolve(&host, id), OK);
        assert_eq!(poll(&host, 300, 301).0, ASYNC_PENDING);
        let (next, context) = REQUEST
            .lock()
            .unwrap()
            .take()
            .expect("second suspended import after resume");
        assert_ne!(next, id);
        assert_eq!(
            context, 300,
            "resumed callbacks use the current frame, not the start frame"
        );
        assert_eq!(resolve(&host, next), OK);
        let (status, bytes) = finish(&host);
        assert_eq!(status, OK, "{}", String::from_utf8_lossy(&bytes));
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            json!({"returned":false})
        );
        let bytes = peer.join().unwrap();
        assert!(bytes.starts_with(b"HTTP/1.1 200"));
        let head = bytes.windows(4).position(|p| p == b"\r\n\r\n").unwrap() + 4;
        assert_eq!(&bytes[head..], b"pong");
    }
}
#[test]
fn p3_http_ffi_cancellation_releases_imports_and_rejects_late_resolution() {
    let host = component(WAT);
    let server = Listener::new();
    let op = server.accept();
    let peer = server.peer();
    assert_eq!(op.wait().0, HTTP_READY);
    assert_eq!(start(&host, &op, P3, 100, 101).0, ASYNC_PENDING);
    assert_eq!(poll(&host, 200, 201).0, ASYNC_PENDING);
    let (id, _) = REQUEST.lock().unwrap().take().unwrap();
    let mut out = Buffer::default();
    assert_eq!(
        unsafe { perry_component_host_dispose(host.0, 400, &mut out) },
        OK
    );
    unsafe { take_output(out) };
    assert_eq!(resolve(&host, id), ERROR);
    assert_eq!(poll(&host, 500, 501).0, ERROR);
    let _ = peer.join().unwrap();
    assert!(unsafe { (*bridge(host.0).unwrap().host.get()).live.is_none() });
}

#[test]
fn p3_http_ffi_listener_close_cancels_a_handler_waiting_on_an_import() {
    let host = component(WAT);
    let server = Listener::new();
    let op = server.accept();
    let peer = server.peer();
    assert_eq!(op.wait().0, HTTP_READY);
    assert_eq!(start(&host, &op, P3, 100, 101).0, ASYNC_PENDING);
    let (id, _) = REQUEST
        .lock()
        .unwrap()
        .take()
        .expect("handler waits on its async import");
    drop(server);
    let (status, bytes) = finish(&host);
    assert_eq!(
        status,
        RELEASED_ERROR,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    assert_eq!(resolve(&host, id), ERROR);
    assert!(unsafe { (*bridge(host.0).unwrap().host.get()).live.is_none() });
    let _ = peer.join().unwrap();
}

#[test]
fn p3_http_ffi_explicit_response_errors_complete_without_retiring_the_host() {
    let mut source=WAT.replace("  (core module $guest", "  (core func $drop-request (canon resource.drop $request))\n  (core module $guest\n    (import \"host\" \"drop-request\" (func $drop-request (param i32)))");
    source = source.replace(
        "(with \"host\" (instance",
        "(with \"host\" (instance (export \"drop-request\" (func $drop-request))",
    );
    let begin = source.find("    (func $original-handle").unwrap();
    let end = source.find("    (func $original-callback").unwrap();
    source.replace_range(begin..end,"    (func $original-handle (param $request i32) (result i32) (call $drop-request (local.get $request)) (call $return (i32.const 1) (i32.const 15) (i32.const 0) (i64.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0)) (i32.const 0))\n");
    let host = component(&source);
    let server = Listener::new();
    for _ in 0..2 {
        let op = server.accept();
        let peer = server.peer();
        assert_eq!(op.wait().0, HTTP_READY);
        assert_eq!(start(&host, &op, P3, 100, 101).0, ASYNC_PENDING);
        for n in 0..2 {
            let (id, _) = REQUEST.lock().unwrap().take().expect("async before import");
            assert_eq!(resolve(&host, id), OK);
            if n == 0 {
                assert_eq!(poll(&host, 200, 201).0, ASYNC_PENDING)
            }
        }
        assert_eq!(finish(&host).0, perry_abi::component_host::HTTP_FAILURE);
        assert!(unsafe { (*bridge(host.0).unwrap().host.get()).live.is_some() });
        assert!(peer.join().unwrap().starts_with(b"HTTP/1.1 500"));
    }
}
