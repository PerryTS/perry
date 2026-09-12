//! Native process fixtures for the OpenCode LSP/formatter lifecycle (#8512).
use super::*;
use std::cell::RefCell;
use std::io::BufRead;
use std::time::Instant;

thread_local! {
    static EVENTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

#[test]
fn native_process_fixture() {
    let Ok(mode) = std::env::var("PERRY_8512_CHILD_MODE") else {
        return;
    };
    if mode == "stalled" {
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    for method in ["initialize", "shutdown"] {
        let mut header = String::new();
        input.read_line(&mut header).unwrap();
        let len: usize = header
            .trim()
            .strip_prefix("Content-Length: ")
            .unwrap()
            .parse()
            .unwrap();
        let mut blank = String::new();
        input.read_line(&mut blank).unwrap();
        assert_eq!(blank, "\r\n");
        let mut body = vec![0; len];
        input.read_exact(&mut body).unwrap();
        assert_eq!(body, method.as_bytes());
        let response = format!("{method}:ok");
        write!(
            output,
            "Content-Length: {}\r\n\r\n{response}",
            response.len()
        )
        .unwrap();
        output.flush().unwrap();
    }
    let mut tail = Vec::new();
    input.read_to_end(&mut tail).unwrap();
    assert_eq!(tail, b"exit");
}

extern "C" fn event(closure: *const ClosureHeader, _a: f64, _b: f64) -> f64 {
    let id = crate::closure::js_closure_get_capture_f64(closure, 0) as usize;
    EVENTS.with(|e| {
        e.borrow_mut()
            .push(["spawn", "exit", "close", "error", "drain"][id].into())
    });
    cp_undefined()
}

extern "C" fn data(_closure: *const ClosureHeader, value: f64) -> f64 {
    OUTPUT.with(|o| o.borrow_mut().extend(cp_value_to_bytes(value)));
    cp_undefined()
}

fn register(target: f64, name: &str, id: usize) {
    crate::closure::js_register_closure_arity(event as *const u8, 2);
    let f = crate::closure::js_closure_alloc(event as *const u8, 1);
    crate::closure::js_closure_set_capture_f64(f, 0, id as f64);
    super::super::emitter::cp_register(target, cp_box_string(name), cp_box_ptr(f.cast()));
}

fn spawn_fixture(mode: &str, timeout: Option<f64>) -> f64 {
    EVENTS.with(|e| e.borrow_mut().clear());
    OUTPUT.with(|o| o.borrow_mut().clear());
    let scope = crate::gc::RuntimeHandleScope::new();
    let env = scope.root_nanbox_f64(cp_box_ptr(crate::object::js_object_alloc(0, 0).cast()));
    for (key, value) in std::env::vars() {
        cp_set_field(env.get_nanbox_f64(), key.as_bytes(), cp_box_string(&value));
    }
    cp_set_field(
        env.get_nanbox_f64(),
        b"PERRY_8512_CHILD_MODE",
        cp_box_string(mode),
    );
    let opts = scope.root_nanbox_f64(cp_box_ptr(crate::object::js_object_alloc(0, 0).cast()));
    cp_set_field(opts.get_nanbox_f64(), b"env", env.get_nanbox_f64());
    cp_set_field(opts.get_nanbox_f64(), b"windowsHide", TAG_TRUE_F64);
    if let Some(timeout) = timeout {
        cp_set_field(opts.get_nanbox_f64(), b"timeout", timeout);
    }
    let mut args = crate::array::js_array_alloc(3);
    for arg in [
        "--exact",
        "child_process::reactor::lifecycle_tests::native_process_fixture",
        "--nocapture",
    ] {
        args = crate::array::js_array_push_f64(args, cp_box_string(arg));
    }
    let path = std::env::current_exe()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let ptr = crate::string::js_string_from_bytes(path.as_ptr(), path.len() as u32);
    let cp = js_child_process_spawn_streams(
        ptr as i64,
        args as i64,
        cp_object_ptr(opts.get_nanbox_f64()).unwrap() as i64,
    );
    assert!(cp_get_field(cp, b"pid") > 0.0);
    for (id, name) in ["spawn", "exit", "close", "error"].iter().enumerate() {
        register(cp, name, id);
    }
    register(cp_get_field(cp, b"stdin"), "drain", 4);
    crate::closure::js_register_closure_arity(data as *const u8, 1);
    let f = crate::closure::js_closure_alloc(data as *const u8, 0);
    super::super::emitter::cp_register(
        cp_get_field(cp, b"stdout"),
        cp_box_string("data"),
        cp_box_ptr(f.cast()),
    );
    cp
}

fn pump_until(handle: u64, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() && Instant::now() < deadline {
        cp_reactor_pump();
        std::thread::sleep(Duration::from_millis(5));
    }
    if !done() {
        cp_live_kill_signal(handle, 9);
    }
    assert!(done(), "process fixture timed out");
}

#[test]
fn lsp_bidirectional_frames_shutdown_and_close_order() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let cp = scope.root_nanbox_f64(spawn_fixture("lsp", None));
    let handle = cp_get_field(cp.get_nanbox_f64(), b"__cpHandle") as u64;
    for method in ["initialize", "shutdown"] {
        let bytes = format!("Content-Length: {}\r\n\r\n{method}", method.len());
        assert!(cp_live_stdin_write(handle, bytes.as_bytes(), None).is_some());
        pump_until(handle, || {
            OUTPUT.with(|o| String::from_utf8_lossy(&o.borrow()).contains(&format!("{method}:ok")))
        });
        assert!(
            cp_live_lock().as_ref().unwrap()[&handle].exited.is_none(),
            "LSP must stay alive between requests"
        );
    }
    cp_live_stdin_write(handle, b"exit", None).unwrap();
    cp_live_stdin_close(handle); // EOF is deferred until the queued exit bytes drain.
    pump_until(handle, || {
        EVENTS.with(|e| e.borrow().iter().any(|v| v == "close"))
    });
    assert_eq!(
        EVENTS.with(|e| e.borrow().clone()),
        ["spawn", "exit", "close"]
    );
    assert_eq!(cp_get_field(cp.get_nanbox_f64(), b"exitCode"), 0.0);
}

#[test]
fn formatter_timeout_runs_while_stdin_is_backpressured() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let cp = scope.root_nanbox_f64(spawn_fixture("stalled", Some(250.0)));
    let handle = cp_get_field(cp.get_nanbox_f64(), b"__cpHandle") as u64;
    let started = Instant::now();
    let outcome = cp_live_stdin_write(handle, &vec![b'x'; 1024 * 1024], None).unwrap();
    assert!(!outcome.below_high_water_mark);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "stdin blocked the event loop"
    );
    pump_until(handle, || {
        EVENTS.with(|e| e.borrow().iter().any(|v| v == "close"))
    });
    assert!(started.elapsed() < Duration::from_secs(5));
    let events = EVENTS.with(|e| e.borrow().clone());
    assert_eq!(events.first().map(String::as_str), Some("spawn"));
    assert_eq!(events.last().map(String::as_str), Some("close"));
    assert_eq!(events.iter().filter(|e| *e == "exit").count(), 1);
    assert_eq!(
        cp_get_field(cp.get_nanbox_f64(), b"killed").to_bits(),
        TAG_TRUE_F64.to_bits()
    );
    assert_eq!(
        cp_value_to_string(cp_get_field(cp.get_nanbox_f64(), b"signalCode")).as_deref(),
        Some("SIGTERM")
    );
}

#[test]
fn abort_cancellation_orders_error_exit_close_and_removes_listener() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let cp = scope.root_nanbox_f64(spawn_fixture("stalled", None));
    let handle = cp_get_field(cp.get_nanbox_f64(), b"__cpHandle") as u64;
    let controller =
        scope.root_nanbox_f64(cp_box_ptr(crate::url::js_abort_controller_new().cast()));
    let signal = scope.root_nanbox_f64(cp_box_ptr(
        crate::url::js_abort_controller_signal(cp_object_ptr(controller.get_nanbox_f64()).unwrap())
            .cast(),
    ));
    cp_install_abort_signal(handle, Some(signal.get_nanbox_f64()), cp_undefined());
    assert_eq!(
        crate::url::abort::js_abort_signal_listener_count(
            cp_object_ptr(signal.get_nanbox_f64()).unwrap()
        ),
        1.0
    );
    crate::url::js_abort_controller_abort(cp_object_ptr(controller.get_nanbox_f64()).unwrap());
    pump_until(handle, || {
        EVENTS.with(|e| e.borrow().iter().any(|v| v == "close"))
    });
    assert_eq!(
        EVENTS.with(|e| e.borrow().clone()),
        ["spawn", "error", "exit", "close"]
    );
    assert_eq!(
        crate::url::abort::js_abort_signal_listener_count(
            cp_object_ptr(signal.get_nanbox_f64()).unwrap()
        ),
        0.0
    );
    assert!(!cp_live_kill_signal(handle, 9));
}
