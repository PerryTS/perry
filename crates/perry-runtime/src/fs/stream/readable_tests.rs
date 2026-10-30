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
