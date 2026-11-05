use super::*;
use crate::ffi::{
    perry_component_driver_http_profile, perry_component_driver_new, perry_component_driver_poll,
    perry_component_host_async_import_resolve,
};
use perry_abi::component_host::ASYNC_PENDING;

const P3: &str = "wasi:http/handler@0.3.0";
static REQUESTS: Mutex<Vec<(u64, u64, u64)>> = Mutex::new(Vec::new());

#[test]
fn persistent_http_profile_selects_fallback_without_executing_an_initializer() {
    let body = r#"
      (core module $m
        (func $start unreachable) (start $start)
        (func (export "handle") (param i32) (result i32) unreachable)
        (func (export "callback") (param i32 i32 i32) (result i32) unreachable))
      (core instance $m (instantiate $m))
      (func $handle async (param "request" u32) (result u32)
        (canon lift (core func $m "handle") async (callback (core func $m "callback"))))
      (instance $handler (export "handle" (func $handle)))
      (export "wasi:http/handler@0.3.0" (instance $handler))"#;
    let world = perry_wit::parse("package wasi:http@0.3.0; interface handler { handle: async func(request: u32) -> u32; } world selected { export handler; }", None).unwrap();
    let contract = serde_json::to_vec(&world).unwrap();
    for nested in [false, true] {
        let text = if nested {
            format!(
                r#"(component (component $inner {body}) (instance $inner (instantiate $inner))
              (alias export $inner "wasi:http/handler@0.3.0" (instance $handler))
              (export "wasi:http/handler@0.3.0" (instance $handler)))"#
            )
        } else {
            format!("(component {body})")
        };
        let wasm = wat::parse_str(text).unwrap();
        let mut out = Buffer::default();
        let status = unsafe {
            perry_component_driver_http_profile(
                wasm.as_ptr(),
                wasm.len(),
                contract.as_ptr(),
                contract.len(),
                &mut out,
            )
        };
        let output = unsafe { take_output(out) };
        assert_eq!(
            status,
            OK,
            "profiling must never run the trapping initializer: {}",
            String::from_utf8_lossy(&output)
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output).unwrap()["concurrent"],
            !nested
        );
    }
}
unsafe extern "C" fn before(
    context: u64,
    origin: u64,
    _: *const u8,
    _: usize,
    name: *const u8,
    len: usize,
    args: *const u8,
    args_len: usize,
    out: *mut Buffer,
) -> i32 {
    assert_eq!(bytes(name, len).unwrap(), b"before");
    let args: serde_json::Value = serde_json::from_slice(bytes(args, args_len).unwrap()).unwrap();
    let request = args["$asyncRequest"].as_str().unwrap().parse().unwrap();
    REQUESTS.lock().unwrap().push((request, origin, context));
    *out = Buffer::default();
    ASYNC_PENDING
}
fn poll(host: &Component, marker: u64) -> serde_json::Value {
    let mut out = Buffer::default();
    let status = unsafe {
        perry_component_driver_poll(host.0, marker, marker + 10000, Some(notify), 42, &mut out)
    };
    let bytes = unsafe { take_output(out) };
    assert_eq!(status, OK, "{}", String::from_utf8_lossy(&bytes));
    serde_json::from_slice(&bytes).unwrap()
}
fn submit(host: &Component, op: &Op) -> u64 {
    let mut out = Buffer::default();
    let status = unsafe {
        perry_component_driver_http_submit(host.0, op.0, P3.as_ptr(), P3.len(), &mut out)
    };
    let bytes = unsafe { take_output(out) };
    assert_eq!(status, OK, "{}", String::from_utf8_lossy(&bytes));
    serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}
fn resume(host: &Component, id: u64) {
    let mut out = Buffer::default();
    let status = unsafe {
        perry_component_host_async_import_resolve(host.0, id, true, b"null".as_ptr(), 4, &mut out)
    };
    let bytes = unsafe { take_output(out) };
    assert_eq!(status, OK, "{}", String::from_utf8_lossy(&bytes));
}

#[test]
fn persistent_http_ffi_defers_callbacks_and_completes_reverse_order_under_fresh_origin_frames() {
    crate::http::tests::guarded(|| {
        REQUESTS.lock().unwrap().clear();
        let wasm =
            wat::parse_str(include_str!("../../../http/p3/concurrent.component.wat")).unwrap();
        let world = perry_wit::parse(
            include_str!("../../../../../../test-wasi/components/http-p3/world.wit"),
            None,
        )
        .unwrap();
        let contract = serde_json::to_vec(&world).unwrap();
        let options = br#"{"wasi":{}}"#;
        let mut out = Buffer::default();
        let status = unsafe {
            perry_component_driver_http_profile(
                wasm.as_ptr(),
                wasm.len(),
                contract.as_ptr(),
                contract.len(),
                &mut out,
            )
        };
        let profile = unsafe { take_output(out) };
        assert_eq!(status, OK);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&profile).unwrap()["concurrent"],
            true
        );
        let mut out = Buffer::default();
        let pointer = unsafe {
            perry_component_driver_new(
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
        assert!(!pointer.is_null(), "{}", String::from_utf8_lossy(&bytes));
        let host = Component(pointer);
        let a = Listener::new();
        let b = Listener::new();
        let peer_a = a.peer();
        let peer_b = b.peer();
        let op_a = a.accept();
        let op_b = b.accept();
        assert_eq!(op_a.wait().0, HTTP_READY);
        assert_eq!(op_b.wait().0, HTTP_READY);
        let first = submit(&host, &op_a);
        let second = submit(&host, &op_b);
        assert!(
            REQUESTS.lock().unwrap().is_empty(),
            "submission must not enter guest callbacks"
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut marker = 100;
        let mut contexts = Vec::new();
        let mut first_gate = None;
        for selected in [second, first] {
            let mut resumed = 0;
            loop {
                marker += 1;
                let report = poll(&host, marker);
                assert_eq!(report["retired"], false, "{report}");
                assert!(report["error"].is_null(), "{report}");
                let requests = std::mem::take(&mut *REQUESTS.lock().unwrap());
                for (request, origin, context) in requests {
                    assert!(origin == first || origin == second);
                    if origin == selected {
                        contexts.push((origin, context));
                        resume(&host, request);
                        resumed += 1;
                    } else {
                        assert_eq!(origin, first);
                        assert!(first_gate.replace((request, origin, context)).is_none());
                    }
                }
                if selected == first {
                    if let Some((request, origin, context)) = first_gate.take() {
                        contexts.push((origin, context));
                        resume(&host, request);
                        resumed += 1;
                    }
                }
                if let Some(event) = report["completed"].as_array().unwrap().first() {
                    assert_eq!(event["id"], selected.to_string());
                    assert_eq!(event["ok"], true, "{report}");
                    assert_eq!(resumed, 2);
                    if selected == second {
                        assert_eq!(report["active"], true);
                        assert!(report["retained"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(
                                |id| id.as_str().and_then(|s| s.parse::<u64>().ok()) == Some(first)
                            ));
                        assert!(!report["retained"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|id| id.as_str().and_then(|s| s.parse::<u64>().ok())
                                == Some(second)));
                    }
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "persistent HTTP completion watchdog: {report}"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        for origin in [first, second] {
            let frames = contexts
                .iter()
                .filter(|(id, _)| *id == origin)
                .map(|(_, frame)| *frame)
                .collect::<Vec<_>>();
            assert_eq!(frames.len(), 2);
            assert_ne!(frames[0], frames[1]);
        }
        assert!(peer_b.join().unwrap().ends_with(b"pong"));
        assert!(peer_a.join().unwrap().ends_with(b"pong"));
        assert_eq!(poll(&host, marker + 1)["active"], false);
    });
}
