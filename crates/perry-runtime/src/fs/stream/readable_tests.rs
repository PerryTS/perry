use super::*;

thread_local! {
    static EVENTS: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

extern "C" fn capture_event(closure: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
    let event = crate::closure::js_closure_get_capture_f64(closure, 0) as u8;
    EVENTS.with(|events| events.borrow_mut().push(event));
    undefined_value()
}

fn drain_turns() {
    for _ in 0..8 {
        crate::promise::js_promise_run_microtasks();
        crate::timer::js_callback_timer_tick();
    }
}

fn write_file_consumes_readable(auto_close: bool) {
    let _global = crate::gc::global_side_table_test_lock();
    EVENTS.with(|events| events.borrow_mut().clear());
    let dir = std::env::temp_dir().join(format!(
        "perry-pr13545-runtime-{}-{auto_close}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("input.txt");
    let output = dir.join("output.txt");
    std::fs::write(&input, b"caller-owned bytes").unwrap();
    let fd = js_fs_open_sync(
        string_value(input.to_str().unwrap().as_bytes()),
        string_value(b"r"),
    );
    let scope = crate::gc::RuntimeHandleScope::new();
    let options = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
    set_object_field_rooted(&scope, &options, b"fd", fd);
    set_object_field_rooted(&scope, &options, b"autoClose", bool_value(auto_close));
    set_object_field_rooted(&scope, &options, b"highWaterMark", 4.0);
    let stream = scope.root_nanbox_f64(js_fs_create_read_stream(
        string_value(b""),
        options.get_nanbox_f64(),
    ));
    for (event, marker) in [(b"end".as_slice(), 1.0), (b"close".as_slice(), 2.0)] {
        let listener =
            scope.root_raw_mut_ptr(js_closure_alloc(crate::fn_info!(capture_event, 0), 1));
        listener.with_mut_ptr(|listener| {
            crate::closure::js_closure_set_capture_f64(listener, 0, marker)
        });
        call_js_method2(
            stream.get_nanbox_f64(),
            b"on",
            string_value(event),
            listener.with_mut_ptr(|listener: *mut ClosureHeader| {
                crate::value::js_nanbox_pointer(listener as i64)
            }),
        );
    }
    let output_value = scope.root_nanbox_f64(string_value(output.to_str().unwrap().as_bytes()));
    let result = unsafe {
        write_file_path_or_fd_result(
            output_value.get_nanbox_f64(),
            stream.get_nanbox_f64(),
            undefined_value(),
        )
    };
    drain_turns();
    let events = EVENTS.with(|events| events.borrow().clone());
    let fd_alive = fd_is_registered(fd as i32);
    let bytes = std::fs::read(&output).unwrap();
    if fd_alive {
        assert_eq!(js_fs_close_sync(fd), 1);
    }
    call_js_method0(stream.get_nanbox_f64(), b"destroy");
    drain_turns();
    assert!(result.is_ok());
    assert_eq!(bytes, b"caller-owned bytes");
    assert_eq!(
        fd_alive, !auto_close,
        "autoClose must control descriptor ownership at EOF"
    );
    assert_eq!(
        events,
        if auto_close { vec![1, 2] } else { vec![1] },
        "end fires once before an optional close"
    );
}

#[test]
fn write_file_preserves_caller_fd_after_readable_eof() {
    write_file_consumes_readable(false);
}

#[test]
fn write_file_closes_owned_fd_after_readable_end() {
    write_file_consumes_readable(true);
}

thread_local! {
    static READABLE_READY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

extern "C" fn note_readable(_closure: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
    READABLE_READY.with(|ready| ready.set(true));
    undefined_value()
}

#[test]
fn write_file_preserves_buffered_bytes_after_a_partial_read() {
    let _global = crate::gc::global_side_table_test_lock();
    READABLE_READY.with(|ready| ready.set(false));
    let dir = std::env::temp_dir().join(format!("perry-pr13545-buffered-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("input.txt");
    let output = dir.join("output.txt");
    let original = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ012345";
    std::fs::write(&input, original).unwrap();
    let scope = crate::gc::RuntimeHandleScope::new();
    let options = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
    set_object_field_rooted(&scope, &options, b"highWaterMark", 16.0);
    let stream = scope.root_nanbox_f64(js_fs_create_read_stream(
        string_value(input.to_str().unwrap().as_bytes()),
        options.get_nanbox_f64(),
    ));
    let listener = scope.root_raw_mut_ptr(js_closure_alloc(crate::fn_info!(note_readable, 0), 0));
    call_js_method2(
        stream.get_nanbox_f64(),
        b"once",
        string_value(b"readable"),
        listener.with_mut_ptr(|listener: *mut ClosureHeader| {
            crate::value::js_nanbox_pointer(listener as i64)
        }),
    );
    for _ in 0..32 {
        crate::promise::js_promise_run_microtasks();
        crate::timer::js_callback_timer_tick();
        if READABLE_READY.with(|ready| ready.get()) {
            break;
        }
    }
    assert!(
        READABLE_READY.with(|ready| ready.get()),
        "the real source must have delivered buffered data"
    );
    let prefix = scope.root_nanbox_f64(call_js_method1(stream.get_nanbox_f64(), b"read", 1.0));
    assert_eq!(bytes_from_buffer_value(prefix.get_nanbox_f64()), b"A");
    let output_value = scope.root_nanbox_f64(string_value(output.to_str().unwrap().as_bytes()));
    let result = unsafe {
        write_file_path_or_fd_result(
            output_value.get_nanbox_f64(),
            stream.get_nanbox_f64(),
            undefined_value(),
        )
    };
    drain_turns();
    let bytes = std::fs::read(&output).unwrap();
    call_js_method0(stream.get_nanbox_f64(), b"destroy");
    drain_turns();
    assert!(result.is_ok());
    assert_eq!(std::fs::read(&input).unwrap(), original);
    assert_eq!(
        bytes, b"BCDEFGHIJKLMNOPQRSTUVWXYZ012345",
        "writeFile must consume buffered and future bytes, not only the descriptor remainder"
    );
    assert_eq!(1 + bytes.len(), original.len());
}

const DEFERRED_INPUT: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ012345abcdefghijklmnopqrstuvwxyz6789+-";

fn deferred_test_dir(subject: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("perry-deferred-{subject}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn wait_for_deferred(promise: f64) -> i32 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let promise = scope.root_nanbox_f64(promise);
    for _ in 0..512 {
        crate::event_pump::perry_poll();
        crate::timer::js_callback_timer_tick();
        let state = crate::promise::js_promise_state(crate::value::js_nanbox_get_pointer(
            promise.get_nanbox_f64(),
        ) as *mut crate::promise::Promise);
        if state != 0 {
            return state;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    0
}

fn watch_deferred_source(scope: &crate::gc::RuntimeHandleScope, stream: f64) {
    let stream = scope.root_nanbox_f64(stream);
    for (event, marker) in [(b"end".as_slice(), 1.0), (b"close".as_slice(), 2.0)] {
        let listener =
            scope.root_raw_mut_ptr(js_closure_alloc(crate::fn_info!(capture_event, 0), 1));
        listener.with_mut_ptr(|listener| {
            crate::closure::js_closure_set_capture_f64(listener, 0, marker)
        });
        call_js_method2(
            stream.get_nanbox_f64(),
            b"on",
            string_value(event),
            listener.with_mut_ptr(|listener: *mut ClosureHeader| {
                crate::value::js_nanbox_pointer(listener as i64)
            }),
        );
    }
}

fn public_deferred_case(preparation: u8, auto_close: bool) {
    let _global = crate::gc::global_side_table_test_lock();
    EVENTS.with(|events| events.borrow_mut().clear());
    READABLE_READY.with(|ready| ready.set(false));
    let dir = deferred_test_dir("readable");
    let input = dir.join("input.txt");
    let output = dir.join("output.txt");
    std::fs::write(&input, DEFERRED_INPUT).unwrap();
    let scope = crate::gc::RuntimeHandleScope::new();
    let path = scope.root_nanbox_f64(string_value(input.to_str().unwrap().as_bytes()));
    let fd = js_fs_open_sync(path.get_nanbox_f64(), string_value(b"r"));
    let options = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
    set_object_field_rooted(&scope, &options, b"fd", fd);
    set_object_field_rooted(&scope, &options, b"autoClose", bool_value(auto_close));
    set_object_field_rooted(&scope, &options, b"highWaterMark", 16.0);
    let stream = scope.root_nanbox_f64(js_fs_create_read_stream(
        string_value(b""),
        options.get_nanbox_f64(),
    ));
    watch_deferred_source(&scope, stream.get_nanbox_f64());
    let prefix = if preparation == 2 {
        let listener =
            scope.root_raw_mut_ptr(js_closure_alloc(crate::fn_info!(note_readable, 0), 0));
        call_js_method2(
            stream.get_nanbox_f64(),
            b"once",
            string_value(b"readable"),
            listener.with_mut_ptr(|listener: *mut ClosureHeader| {
                crate::value::js_nanbox_pointer(listener as i64)
            }),
        );
        for _ in 0..64 {
            crate::promise::js_promise_run_microtasks();
            crate::timer::js_callback_timer_tick();
            if READABLE_READY.with(|ready| ready.get()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(READABLE_READY.with(|ready| ready.get()));
        let first = call_js_method1(stream.get_nanbox_f64(), b"read", 1.0);
        assert_eq!(bytes_from_buffer_value(first), b"A");
        1
    } else {
        if preparation == 1 {
            drain_turns();
        }
        assert_eq!(
            crate::node_stream::js_node_stream_method_readable_length(
                crate::value::js_nanbox_get_pointer(stream.get_nanbox_f64())
            ),
            0.0
        );
        0
    };
    let output_path = scope.root_nanbox_f64(string_value(output.to_str().unwrap().as_bytes()));
    let promise = scope.root_nanbox_f64(crate::fs::defer_write_file_promise(
        output_path.get_nanbox_f64(),
        stream.get_nanbox_f64(),
        undefined_value(),
    ));
    assert_eq!(
        crate::promise::js_promise_state(crate::value::js_nanbox_get_pointer(
            promise.get_nanbox_f64()
        ) as *mut crate::promise::Promise),
        0
    );
    let state = wait_for_deferred(promise.get_nanbox_f64());
    let at_completion = EVENTS.with(|events| events.borrow().clone());
    drain_turns();
    let events = EVENTS.with(|events| events.borrow().clone());
    let alive = fd_is_registered(fd as i32);
    let fd_usable = alive && object_ptr_from_value(crate::fs::js_fs_fstat_sync(fd)).is_some();
    let actual = std::fs::read(&output).unwrap();
    let unchanged = std::fs::read(&input).unwrap();
    call_js_method0(stream.get_nanbox_f64(), b"destroy");
    drain_turns();
    assert_eq!(state, 1, "the public deferred write must fulfill");
    assert_eq!(
        at_completion.iter().filter(|event| **event == 1).count(),
        1,
        "writeFile must not fulfill before the source's real end"
    );
    let expected: &[u8] = if prefix == 1 {
        b"BCDEFGHIJKLMNOPQRSTUVWXYZ012345abcdefghijklmnopqrstuvwxyz6789+-"
    } else {
        DEFERRED_INPUT
    };
    assert_eq!(
        actual, expected,
        "the deferred consumer must retain buffered and future bytes"
    );
    assert_eq!(actual.len() + prefix, 64);
    assert_eq!(unchanged, DEFERRED_INPUT);
    assert_eq!(alive, !auto_close);
    assert_eq!(fd_usable, !auto_close);
    assert_eq!(events, if auto_close { vec![1, 2] } else { vec![1] });
}

#[test]
fn public_deferred_write_file_immediate_borrowed() {
    public_deferred_case(0, false);
}
#[test]
fn public_deferred_write_file_immediate_owned() {
    public_deferred_case(0, true);
}
#[test]
fn public_deferred_write_file_idle_borrowed() {
    public_deferred_case(1, false);
}
#[test]
fn public_deferred_write_file_idle_owned() {
    public_deferred_case(1, true);
}
#[test]
fn public_deferred_write_file_buffered_borrowed() {
    public_deferred_case(2, false);
}
#[test]
fn public_deferred_write_file_buffered_owned() {
    public_deferred_case(2, true);
}

#[test]
fn public_deferred_write_file_direct_data_remains_deferred() {
    let _global = crate::gc::global_side_table_test_lock();
    let output = deferred_test_dir("direct").join("output.txt");
    let scope = crate::gc::RuntimeHandleScope::new();
    let path = scope.root_nanbox_f64(string_value(output.to_str().unwrap().as_bytes()));
    let data = scope.root_nanbox_f64(string_value(b"direct bytes"));
    let promise = scope.root_nanbox_f64(crate::fs::defer_write_file_promise(
        path.get_nanbox_f64(),
        data.get_nanbox_f64(),
        undefined_value(),
    ));
    assert!(!output.exists());
    assert_eq!(wait_for_deferred(promise.get_nanbox_f64()), 1);
    assert_eq!(std::fs::read(output).unwrap(), b"direct bytes");
}

#[test]
fn public_deferred_write_file_keeps_destination_fd_open() {
    let _global = crate::gc::global_side_table_test_lock();
    let dir = deferred_test_dir("destination-fd");
    let input = dir.join("input.txt");
    let output = dir.join("output.txt");
    std::fs::write(&input, DEFERRED_INPUT).unwrap();
    let scope = crate::gc::RuntimeHandleScope::new();
    let input = scope.root_nanbox_f64(string_value(input.to_str().unwrap().as_bytes()));
    let output_path = scope.root_nanbox_f64(string_value(output.to_str().unwrap().as_bytes()));
    let fd = js_fs_open_sync(output_path.get_nanbox_f64(), string_value(b"w"));
    let stream = scope.root_nanbox_f64(js_fs_create_read_stream(
        input.get_nanbox_f64(),
        undefined_value(),
    ));
    let promise = scope.root_nanbox_f64(crate::fs::defer_write_file_promise(
        fd,
        stream.get_nanbox_f64(),
        undefined_value(),
    ));
    let state = wait_for_deferred(promise.get_nanbox_f64());
    drain_turns();
    let alive = fd_is_registered(fd as i32);
    let bytes = std::fs::read(output).unwrap();
    if alive {
        js_fs_close_sync(fd);
    }
    call_js_method0(stream.get_nanbox_f64(), b"destroy");
    drain_turns();
    assert_eq!(state, 1);
    assert!(alive);
    assert_eq!(bytes, DEFERRED_INPUT);
}

#[test]
fn public_deferred_write_file_rejection_keeps_destination_fd_and_closes_source() {
    let _global = crate::gc::global_side_table_test_lock();
    EVENTS.with(|events| events.borrow_mut().clear());
    let dir = deferred_test_dir("write-error");
    let input = dir.join("input.txt");
    let output = dir.join("output.txt");
    std::fs::write(&input, DEFERRED_INPUT).unwrap();
    std::fs::write(&output, b"unchanged").unwrap();
    let scope = crate::gc::RuntimeHandleScope::new();
    let input_path = scope.root_nanbox_f64(string_value(input.to_str().unwrap().as_bytes()));
    let output_path = scope.root_nanbox_f64(string_value(output.to_str().unwrap().as_bytes()));
    let fd = js_fs_open_sync(output_path.get_nanbox_f64(), string_value(b"r"));
    let stream = scope.root_nanbox_f64(js_fs_create_read_stream(
        input_path.get_nanbox_f64(),
        undefined_value(),
    ));
    watch_deferred_source(&scope, stream.get_nanbox_f64());
    let promise = scope.root_nanbox_f64(crate::fs::defer_write_file_promise(
        fd,
        stream.get_nanbox_f64(),
        undefined_value(),
    ));
    crate::promise::js_promise_mark_internally_handled(crate::value::js_nanbox_get_pointer(
        promise.get_nanbox_f64(),
    ) as *mut crate::promise::Promise);
    let state = wait_for_deferred(promise.get_nanbox_f64());
    if state == 2 {
        crate::promise::js_promise_reason(crate::value::js_nanbox_get_pointer(
            promise.get_nanbox_f64(),
        ) as *mut crate::promise::Promise);
    }
    drain_turns();
    let events = EVENTS.with(|events| events.borrow().clone());
    let alive = fd_is_registered(fd as i32);
    let bytes = std::fs::read(output).unwrap();
    if alive {
        js_fs_close_sync(fd);
    }
    call_js_method0(stream.get_nanbox_f64(), b"destroy");
    drain_turns();
    assert_eq!(
        state, 2,
        "a destination write failure must reject, not report empty success"
    );
    assert!(alive);
    assert_eq!(bytes, b"unchanged");
    assert_eq!(
        events.iter().filter(|event| **event == 2).count(),
        1,
        "iterator cleanup closes the source once"
    );
}

thread_local! {
    static DEFERRED_CALLBACK: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

extern "C" fn deferred_write_callback(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    error: f64,
) -> f64 {
    let success = matches!(
        error.to_bits(),
        crate::value::TAG_UNDEFINED | crate::value::TAG_NULL
    );
    let ended =
        EVENTS.with(|events| events.borrow().iter().filter(|event| **event == 1).count() == 1);
    DEFERRED_CALLBACK.with(|callback| callback.set(Some(success && ended)));
    undefined_value()
}

#[test]
fn public_deferred_write_file_callback_waits_for_source_end() {
    let _global = crate::gc::global_side_table_test_lock();
    EVENTS.with(|events| events.borrow_mut().clear());
    DEFERRED_CALLBACK.with(|callback| callback.set(None));
    let dir = deferred_test_dir("callback");
    let input = dir.join("input.txt");
    let output = dir.join("output.txt");
    std::fs::write(&input, DEFERRED_INPUT).unwrap();
    let scope = crate::gc::RuntimeHandleScope::new();
    let input = scope.root_nanbox_f64(string_value(input.to_str().unwrap().as_bytes()));
    let output_path = scope.root_nanbox_f64(string_value(output.to_str().unwrap().as_bytes()));
    let stream = scope.root_nanbox_f64(js_fs_create_read_stream(
        input.get_nanbox_f64(),
        undefined_value(),
    ));
    watch_deferred_source(&scope, stream.get_nanbox_f64());
    let callback = scope.root_raw_mut_ptr(js_closure_alloc(
        crate::fn_info!(deferred_write_callback, 1; with_declared(1)),
        0,
    ));
    crate::fs::defer_write_file_callback(
        output_path.get_nanbox_f64(),
        stream.get_nanbox_f64(),
        undefined_value(),
        callback.get_raw_const_ptr(),
    );
    assert_eq!(DEFERRED_CALLBACK.with(|callback| callback.get()), None);
    for _ in 0..512 {
        crate::event_pump::perry_poll();
        crate::timer::js_callback_timer_tick();
        if DEFERRED_CALLBACK.with(|callback| callback.get().is_some()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(
        DEFERRED_CALLBACK.with(|callback| callback.get()),
        Some(true)
    );
    assert_eq!(std::fs::read(output).unwrap(), DEFERRED_INPUT);
    drain_turns();
}

#[test]
fn public_deferred_write_file_retains_path_until_terminal_unlink() {
    let _global = crate::gc::global_side_table_test_lock();
    EVENTS.with(|events| events.borrow_mut().clear());
    let dir = deferred_test_dir("unlink-order");
    let input = dir.join("input.txt");
    let output = dir.join("output.txt");
    std::fs::write(&input, DEFERRED_INPUT).unwrap();
    let scope = crate::gc::RuntimeHandleScope::new();
    let input = scope.root_nanbox_f64(string_value(input.to_str().unwrap().as_bytes()));
    let output_path = scope.root_nanbox_f64(string_value(output.to_str().unwrap().as_bytes()));
    let stream = scope.root_nanbox_f64(js_fs_create_read_stream(
        input.get_nanbox_f64(),
        undefined_value(),
    ));
    watch_deferred_source(&scope, stream.get_nanbox_f64());
    let write = scope.root_nanbox_f64(crate::fs::defer_write_file_promise(
        output_path.get_nanbox_f64(),
        stream.get_nanbox_f64(),
        undefined_value(),
    ));
    let unlink = scope.root_nanbox_f64(crate::fs::defer_unlink_promise(
        output_path.get_nanbox_f64(),
    ));
    assert!(crate::fs::has_pending_path_write(
        output_path.get_nanbox_f64()
    ));
    assert_eq!(wait_for_deferred(write.get_nanbox_f64()), 1);
    assert_eq!(
        EVENTS.with(|events| events.borrow().iter().filter(|event| **event == 1).count()),
        1
    );
    assert_eq!(wait_for_deferred(unlink.get_nanbox_f64()), 1);
    assert!(!output.exists());
    assert!(!crate::fs::has_pending_path_write(
        output_path.get_nanbox_f64()
    ));
    drain_turns();
}
