mod p3;
mod persistent;
use super::*;
use serde_json::json;
use std::{
    io::{Read, Write},
    net::{Shutdown, TcpStream},
    sync::{Condvar, Mutex},
    time::{Duration, Instant},
};

static WAKE: (Mutex<u64>, Condvar) = (Mutex::new(0), Condvar::new());
unsafe extern "C" fn notify(agent: u64) {
    assert_eq!(agent, 42);
    *WAKE.0.lock().unwrap() += 1;
    WAKE.1.notify_all();
}
unsafe fn take_output(buffer: Buffer) -> Vec<u8> {
    let data = bytes(buffer.data, buffer.len).unwrap().to_vec();
    perry_component_host_buffer_drop(buffer);
    data
}
struct Listener(*mut c_void, String);
impl Listener {
    fn new() -> Self {
        let options = br#"{"address":"127.0.0.1:0","maxConnections":1,"lingerTimeoutMs":20}"#;
        let mut output = Buffer::default();
        let server = unsafe {
            perry_component_http_server_new(options.as_ptr(), options.len(), &mut output)
        };
        let bytes = unsafe { take_output(output) };
        assert!(!server.is_null(), "{}", String::from_utf8_lossy(&bytes));
        Self(server, serde_json::from_slice(&bytes).unwrap())
    }
    fn accept(&self) -> Op {
        let mut output = Buffer::default();
        let op = unsafe { perry_component_http_accept(self.0, &mut output) };
        let bytes = unsafe { take_output(output) };
        assert!(!op.is_null(), "{}", String::from_utf8_lossy(&bytes));
        Op(op)
    }
    fn peer(&self) -> std::thread::JoinHandle<Vec<u8>> {
        let address = self.1.clone();
        std::thread::spawn(move || {
            let mut socket = TcpStream::connect(address).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            socket
                .write_all(b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4\r\n\r\npong")
                .unwrap();
            socket.shutdown(Shutdown::Write).unwrap();
            let mut bytes = Vec::new();
            socket.read_to_end(&mut bytes).unwrap();
            bytes
        })
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        unsafe {
            perry_component_http_server_drop(self.0);
        }
    }
}
struct Op(*mut c_void);
impl Op {
    fn poll(&self) -> (i32, Vec<u8>) {
        let mut output = Buffer {
            data: std::ptr::null_mut(),
            len: usize::MAX,
        };
        let status = unsafe { perry_component_http_poll(self.0, Some(notify), 42, &mut output) };
        if status != ERROR {
            assert!(output.data.is_null());
            assert_eq!(output.len, 0);
        }
        (status, unsafe { take_output(output) })
    }
    fn wait(&self) -> (i32, Vec<u8>) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let generation = *WAKE.0.lock().unwrap();
            let result = self.poll();
            if result.0 != HTTP_PENDING {
                return result;
            }
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .expect("HTTP FFI wake watchdog");
            let (value, timeout) = WAKE
                .1
                .wait_timeout_while(WAKE.0.lock().unwrap(), remaining, |v| *v == generation)
                .unwrap();
            assert!(
                !timeout.timed_out() || *value != generation,
                "missing native HTTP wake"
            );
        }
    }
    fn invoke(&self, host: &Component, interface: &str) -> (i32, Vec<u8>) {
        let mut out = Buffer::default();
        let status = unsafe {
            perry_component_http_handle(
                host.0,
                self.0,
                interface.as_ptr(),
                interface.len(),
                1,
                99,
                &mut out,
            )
        };
        (status, unsafe { take_output(out) })
    }
}
impl Drop for Op {
    fn drop(&mut self) {
        unsafe {
            perry_component_http_drop(self.0);
        }
    }
}
const INTERFACE: &str = "wasi:http/incoming-handler@0.2.0";
struct Component(*mut c_void);
impl Component {
    fn new(mode: u32) -> Self {
        Self::with_resources(mode, None)
    }
    fn with_resources(mode: u32, callback: Option<ResourceCallback>) -> Self {
        let wasm = wat::parse_str(include_str!(
            "../../../../perry-wasi/src/http/p2/incoming.component.wat"
        ))
        .unwrap();
        let world = perry_wit::parse(r#"package wasi:http@0.2.0;
            interface types { resource incoming-request; resource response-outparam; }
            interface incoming-handler { use types.{incoming-request, response-outparam}; handle: func(request: incoming-request, response-out: response-outparam); }
            world proxy { import types; export incoming-handler; export configure: func(mode: u32); }"#, None).unwrap();
        let contract = serde_json::to_vec(&world).unwrap();
        let options = br#"{"wasi":{}}"#;
        let mut output = Buffer::default();
        let handle = unsafe {
            perry_component_host_new(
                wasm.as_ptr(),
                wasm.len(),
                contract.as_ptr(),
                contract.len(),
                options.as_ptr(),
                options.len(),
                None,
                Some(perry_component_host_buffer_drop),
                callback,
                1,
                &mut output,
            )
        };
        let bytes = unsafe { take_output(output) };
        assert!(!handle.is_null(), "{}", String::from_utf8_lossy(&bytes));
        let args = serde_json::to_vec(&json!([mode])).unwrap();
        let mut output = Buffer::default();
        let status = unsafe {
            perry_component_host_call(
                handle,
                std::ptr::null(),
                0,
                b"configure".as_ptr(),
                9,
                args.as_ptr(),
                args.len(),
                1,
                &mut output,
            )
        };
        let bytes = unsafe { take_output(output) };
        assert_eq!(status, OK, "{}", String::from_utf8_lossy(&bytes));
        Self(handle)
    }
}
impl Drop for Component {
    fn drop(&mut self) {
        unsafe {
            perry_component_host_drop(self.0);
        }
    }
}

#[test]
fn http_ffi_accept_is_nonblocking_and_wakes_for_canonical_handler_and_completion() {
    let server = Listener::new();
    let op = server.accept();
    assert_eq!(op.poll(), (HTTP_PENDING, Vec::new()));
    let component = Component::new(0);
    // Invoking before a request exists neither consumes the accept nor retires the Store.
    assert_eq!(op.invoke(&component, INTERFACE).0, ERROR);
    let peer = server.peer();
    assert_eq!(op.wait(), (HTTP_READY, Vec::new()));
    assert_eq!(op.poll(), (HTTP_READY, Vec::new()));
    assert_eq!(op.invoke(&component, INTERFACE), (OK, Vec::new()));
    assert_eq!(op.wait(), (OK, Vec::new()));
    assert_eq!(op.poll().0, ERROR);
    let bytes = peer.join().unwrap();
    assert!(bytes.starts_with(b"HTTP/1.1 202"));
    assert!(bytes.ends_with(b"4\r\npong\r\n0\r\n\r\n"));
    assert!(unsafe {
        bridge(component.0)
            .unwrap()
            .host
            .get()
            .as_ref()
            .unwrap()
            .live
            .is_some()
    });
}
#[test]
fn http_ffi_reports_retirement_separately_from_response_errors() {
    for mode in [1, 2, 3, 4] {
        let server = Listener::new();
        let op = server.accept();
        let component = Component::new(mode);
        let peer = server.peer();
        assert_eq!(op.wait().0, HTTP_READY);
        let (status, _) = op.invoke(&component, INTERFACE);
        assert_eq!(status, if mode == 3 { OK } else { RELEASED_ERROR });
        assert_eq!(op.wait().0, ERROR);
        assert!(peer.join().unwrap().starts_with(b"HTTP/1.1 500"));
        assert_eq!(
            unsafe { (*bridge(component.0).unwrap().host.get()).live.is_some() },
            mode == 3
        );
    }
}
#[test]
fn http_ffi_reentry_does_not_consume_request_and_bad_export_does_not_retire_host() {
    let server = Listener::new();
    let op = server.accept();
    let component = Component::new(0);
    let peer = server.peer();
    assert_eq!(op.wait().0, HTTP_READY);
    unsafe {
        bridge(component.0).unwrap().busy.set(true);
    }
    assert_eq!(op.invoke(&component, INTERFACE).0, ERROR);
    unsafe {
        bridge(component.0).unwrap().busy.set(false);
        operation(op.0).unwrap().busy.set(true);
    }
    assert_eq!(op.invoke(&component, INTERFACE).0, ERROR);
    assert_eq!(op.poll().0, ERROR);
    unsafe {
        operation(op.0).unwrap().busy.set(false);
    }
    assert_eq!(op.poll().0, HTTP_READY);
    assert_eq!(op.invoke(&component, "missing").0, ERROR);
    assert_eq!(op.wait().0, ERROR);
    assert!(peer.join().unwrap().starts_with(b"HTTP/1.1 500"));
    assert!(unsafe { (*bridge(component.0).unwrap().host.get()).live.is_some() });
}
#[test]
fn http_ffi_server_drop_cancels_pending_and_ready_operations_without_guest_calls() {
    for ready in [false, true] {
        let server = Listener::new();
        let op = server.accept();
        let mut socket = if ready {
            let mut socket = TcpStream::connect(&server.1).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            socket
                .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .unwrap();
            assert_eq!(op.wait().0, HTTP_READY);
            Some(socket)
        } else {
            assert_eq!(op.poll().0, HTTP_PENDING);
            None
        };
        drop(server);
        if ready {
            // Closure invalidates a ready request too; it cannot invoke guest
            // code after its server has been closed.
            assert_eq!(op.wait().0, ERROR);
            drop(op);
            assert_eq!(socket.as_mut().unwrap().read(&mut [0]).unwrap(), 0);
        } else {
            assert_eq!(op.wait().0, ERROR);
        }
    }
}
#[test]
fn http_ffi_rejects_missing_authority_unknown_options_and_invalid_credentials() {
    for options in [
        json!({}),
        json!({"address":"localhost:80"}),
        json!({"address":"127.0.0.1:0","maxConnections":0}),
        json!({"address":"127.0.0.1:0","inheritTls":true}),
        json!({"address":"127.0.0.1:0","tls":{"certificatePem":"invalid", "privateKeyPem":"invalid"}}),
    ] {
        let bytes = serde_json::to_vec(&options).unwrap();
        let mut output = Buffer::default();
        assert!(unsafe {
            perry_component_http_server_new(bytes.as_ptr(), bytes.len(), &mut output)
        }
        .is_null());
        assert!(!unsafe { take_output(output) }.is_empty());
    }
    let mut output = Buffer::default();
    assert_eq!(
        unsafe { perry_component_http_poll(std::ptr::null_mut(), Some(notify), 42, &mut output) },
        ERROR
    );
    assert!(!unsafe { take_output(output) }.is_empty());
}

#[test]
fn http_ffi_retirement_uses_a_separate_cleanup_exception_context() {
    static CLEANUP_CONTEXT: AtomicU64 = AtomicU64::new(0);
    unsafe extern "C" fn resource_callback(
        context: u64,
        event: *const u8,
        len: usize,
        out: *mut Buffer,
    ) -> i32 {
        let value: serde_json::Value = serde_json::from_slice(bytes(event, len).unwrap()).unwrap();
        assert_eq!(value["kind"], "drop");
        CLEANUP_CONTEXT.store(context, Ordering::Release);
        output(out, b"cleanup destructor threw".to_vec());
        ERROR
    }
    let server = Listener::new();
    let op = server.accept();
    let component = Component::with_resources(2, Some(resource_callback));
    unsafe {
        let state = (*bridge(component.0).unwrap().host.get())
            .live
            .as_mut()
            .unwrap()
            .store
            .data_mut();
        // Models an owned host payload retained by a guest. Store retirement
        // must deliver its destructor through the real C callback bridge.
        state.host_resources.entries.insert(
            "retained".into(),
            crate::host_resources::Entry {
                reference: crate::ResourceRef {
                    token: "retained".into(),
                    type_key: "test:retained/token".into(),
                    generation: 1,
                },
                rep: 1,
                ty: 0,
                handle: None,
            },
        );
    }
    let peer = server.peer();
    assert_eq!(op.wait().0, HTTP_READY);
    let (status, bytes) = op.invoke(&component, INTERFACE);
    assert_eq!(status, RELEASED_ERROR);
    assert!(String::from_utf8_lossy(&bytes).contains("trapped"));
    assert!(!String::from_utf8_lossy(&bytes).contains("cleanup destructor threw"));
    assert_eq!(CLEANUP_CONTEXT.load(Ordering::Acquire), 99);
    assert_eq!(op.wait().0, ERROR);
    assert!(peer.join().unwrap().starts_with(b"HTTP/1.1 500"));
}
