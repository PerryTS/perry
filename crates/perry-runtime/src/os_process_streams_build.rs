//! `build_stream_object_with_write`: the shape/field builder behind the
//! `process.stdin` / `process.stdout` / `process.stderr` stream singletons.
//! Split out of `os_process_streams.rs` to stay under the 2,000-line cap
//! (#10750).

use super::*;

/// Build a stream object with a `write` field bound to the given stub.
pub(super) fn build_stream_object_with_write(
    write_info: *const crate::closure::JsFunctionInfo,
    fd: f64,
    writable: f64,
) -> *mut crate::object::ObjectHeader {
    use crate::closure::js_closure_alloc;
    use crate::object::{js_object_alloc_with_shape, js_object_set_field};
    use crate::value::JSValue;

    let fd_i = fd as i32;
    let is_tty = crate::tty::is_tty_fd(fd_i);
    if is_tty {
        crate::tty::attach_tty_constructor_prototype(
            crate::object::bound_native_callable_export_value(
                "tty",
                if fd_i == 0 {
                    "ReadStream"
                } else {
                    "WriteStream"
                },
            ),
            if fd_i == 0 {
                "ReadStream"
            } else {
                "WriteStream"
            },
        );
    }

    // #3962: EventEmitter listener-removal + lifecycle surface appended to the
    // stdin shapes. The TTY *write* stream keeps its existing shape; generic
    // non-TTY streams keep `main`'s no-op teardown surface.
    const STDIN_TEARDOWN_KEYS: &[u8] =
        b"addListener\0removeListener\0off\0removeAllListeners\0pause\0resume\0unref\0ref\0destroy\0setEncoding\0";
    const GENERIC_TEARDOWN_KEYS: &[u8] =
        b"addListener\0removeListener\0off\0removeAllListeners\0pause\0resume\0unref\0destroy\0";
    // #11418: Writable buffer state, appended last on stdout/stderr (fields
    // field_count-3..). `write` is synchronous, so nothing is ever queued.
    const WRITABLE_STATE_KEYS: &[u8] =
        b"writableLength\0writableHighWaterMark\0writableNeedDrain\0";
    let is_stdin = fd_i == 0;
    let (class_id, packed, field_count, teardown_start): (u32, Vec<u8>, u32, Option<u32>) =
        if is_stdin {
            let mut keys = b"write\0fd\0emit\0on\0once\0writable\0readable\0readableEnded\0destroyed\0closed\0isRaw\0isTTY\0".to_vec();
            keys.extend_from_slice(STDIN_TEARDOWN_KEYS);
            keys.extend_from_slice(b"read\0"); // field 22: Readable.read()
            keys.extend_from_slice(b"listeners\0"); // field 23: EventEmitter.listeners()
            (
                if is_tty {
                    crate::tty::CLASS_ID_TTY_READ_STREAM
                } else {
                    0
                },
                keys,
                24,
                Some(12),
            )
        } else if is_tty {
            (
                crate::tty::CLASS_ID_TTY_WRITE_STREAM,
                [&b"write\0fd\0emit\0on\0once\0writable\0addListener\0removeListener\0off\0removeAllListeners\0"[..], WRITABLE_STATE_KEYS].concat(),
                13,
                None,
            )
        } else {
            let mut keys = b"write\0fd\0emit\0on\0once\0writable\0".to_vec();
            keys.extend_from_slice(GENERIC_TEARDOWN_KEYS);
            keys.extend_from_slice(WRITABLE_STATE_KEYS);
            (0, keys, 17, Some(6))
        };
    let obj = if class_id == 0 {
        // Shape ids must stay clear of NAVIGATOR_CLASS_ID (0x7FFF_FF22) — the
        // per-shape key registry is first-registration-wins, so sharing an id
        // with navigator made `process.stdout.write` resolve to undefined
        // whenever navigator was built first. stdin gets its own id because
        // its key layout diverges from stdout/stderr past field 5.
        let shape_id = if is_stdin { 0x7FFF_FF29 } else { 0x7FFF_FF23 };
        js_object_alloc_with_shape(shape_id, field_count, packed.as_ptr(), packed.len() as u32)
    } else {
        crate::object::js_object_alloc_class_with_keys(
            class_id,
            0,
            field_count,
            packed.as_ptr(),
            packed.len() as u32,
        )
    };
    // `write` takes up to three positional args — `write(chunk[, encoding][,
    // callback])`. The caller's info declares all three, so dispatch
    // pads/truncates to exactly the three the stub takes; sizing the call to
    // the call site would drop the trailing callback (#6672).
    let closure = js_closure_alloc(write_info, 0);
    let cval = JSValue::pointer(closure as *const u8);
    js_object_set_field(obj, 0, cval);
    js_object_set_field(obj, 1, JSValue::number(fd));
    let emit = js_closure_alloc(crate::fn_info!(process_stream_emit_stub, 1), 0);
    js_object_set_field(obj, 2, JSValue::pointer(emit as *const u8));
    if is_tty && fd_i != 0 {
        js_object_set_field(
            obj,
            3,
            JSValue::from_bits(crate::tty::tty_listener_on_value().to_bits()),
        );
        js_object_set_field(
            obj,
            4,
            JSValue::from_bits(crate::tty::tty_listener_on_value().to_bits()),
        );
    } else if is_stdin {
        // Real `on(event, cb)` so `process.stdin.on("data"/"readable", …)`
        // registers a keyboard listener instead of dropping it (#input).
        let on = stdin_native_method(
            crate::fn_info!(process_stdin_on, 2; with_declared(2)),
            "on",
            2,
        );
        js_object_set_field(obj, 3, JSValue::from_bits(on.to_bits()));
        // `once` routes through the same registry as `on`/`addListener` so a
        // one-shot listener registered on an aliased binding is not dropped either.
        let once = stdin_native_method(
            crate::fn_info!(process_stdin_add_listener_once, 2; with_declared(2)),
            "once",
            2,
        );
        js_object_set_field(obj, 4, JSValue::from_bits(once.to_bits()));
    } else {
        let on = js_closure_alloc(crate::fn_info!(process_stream_on_once_stub, 1), 0);
        js_object_set_field(obj, 3, JSValue::pointer(on as *const u8));
        let once = js_closure_alloc(crate::fn_info!(process_stream_on_once_stub, 1), 0);
        js_object_set_field(obj, 4, JSValue::pointer(once as *const u8));
    }
    js_object_set_field(obj, 5, JSValue::from_bits(writable.to_bits()));
    if !is_stdin {
        js_object_set_field(obj, field_count - 3, JSValue::number(0.0));
        js_object_set_field(obj, field_count - 2, JSValue::number(65536.0));
        js_object_set_field(
            obj,
            field_count - 1,
            JSValue::from_bits(crate::value::TAG_FALSE),
        );
    }
    if fd_i == 0 {
        js_object_set_field(obj, 6, JSValue::from_bits(crate::value::TAG_TRUE));
        js_object_set_field(obj, 7, JSValue::from_bits(crate::value::TAG_FALSE));
        js_object_set_field(obj, 8, JSValue::from_bits(crate::value::TAG_FALSE));
        js_object_set_field(obj, 9, JSValue::from_bits(crate::value::TAG_FALSE));
        js_object_set_field(obj, 10, JSValue::from_bits(crate::value::TAG_FALSE));
        js_object_set_field(
            obj,
            11,
            JSValue::from_bits(if is_tty {
                crate::value::TAG_TRUE
            } else {
                crate::value::TAG_FALSE
            }),
        );
    } else if is_tty {
        js_object_set_field(
            obj,
            6,
            JSValue::from_bits(crate::tty::tty_listener_on_value().to_bits()),
        );
        js_object_set_field(
            obj,
            7,
            JSValue::from_bits(crate::tty::tty_listener_remove_value().to_bits()),
        );
        js_object_set_field(
            obj,
            8,
            JSValue::from_bits(crate::tty::tty_listener_remove_value().to_bits()),
        );
        js_object_set_field(
            obj,
            9,
            JSValue::from_bits(crate::tty::tty_listener_remove_all_value().to_bits()),
        );
    }
    // #3962: install the appended listener-removal + lifecycle methods. stdin
    // replaces the stream stubs below with its real listener/flow operations;
    // stdout and stderr retain the stubs.
    if let Some(start) = teardown_start {
        let on_once = crate::fn_info!(process_stream_on_once_stub, 1);
        let set_field_with_stub = |idx: u32, stub: *const crate::closure::JsFunctionInfo| {
            let c = js_closure_alloc(stub, 0);
            js_object_set_field(obj, idx, JSValue::pointer(c as *const u8));
        };
        let lifecycle = if is_stdin {
            crate::fn_info!(process_stdin_detach_stub, 1)
        } else {
            on_once
        };
        // On stdin these must be REAL: a TUI registers its keyboard through an
        // aliased binding (`stdin.addListener("readable", handler)`), which lands
        // here rather than on codegen's direct `process.stdin.on(...)` extern. As
        // no-op stubs they silently discarded the handler.
        if is_stdin {
            let add = stdin_native_method(
                crate::fn_info!(process_stdin_add_listener, 2; with_declared(2)),
                "addListener",
                2,
            );
            js_object_set_field(obj, start, JSValue::from_bits(add.to_bits()));
            let rm = stdin_native_method(&PROCESS_STDIN_REMOVE_LISTENER_INFO, "removeListener", 2);
            js_object_set_field(obj, start + 1, JSValue::from_bits(rm.to_bits()));
            let off = stdin_native_method(&PROCESS_STDIN_REMOVE_LISTENER_INFO, "off", 2);
            js_object_set_field(obj, start + 2, JSValue::from_bits(off.to_bits()));
        } else {
            set_field_with_stub(start, on_once); // addListener
            set_field_with_stub(start + 1, on_once); // removeListener
            set_field_with_stub(start + 2, on_once); // off
        }
        if is_stdin {
            let remove_all = stdin_native_method(
                crate::fn_info!(process_stdin_remove_all_listeners, 1; with_declared(1)),
                "removeAllListeners",
                1,
            );
            js_object_set_field(obj, start + 3, JSValue::from_bits(remove_all.to_bits()));
        } else {
            set_field_with_stub(start + 3, on_once);
        }
        set_field_with_stub(start + 4, lifecycle); // pause
                                                   // resume: real flowing-mode start on stdin, no-op on stdout/stderr.
        set_field_with_stub(
            start + 5,
            if is_stdin {
                crate::fn_info!(process_stdin_resume, 1)
            } else {
                on_once
            },
        ); // resume
           // #9676: on stdin, `unref`/`ref` are a SYMMETRIC pair that only moves
           // the event-loop hold; on stdout/stderr `unref` stays the shared no-op.
        set_field_with_stub(
            start + 6,
            if is_stdin {
                crate::fn_info!(process_stdin_unref_stub, 1)
            } else {
                on_once
            },
        ); // unref
        if is_stdin {
            set_field_with_stub(start + 7, crate::fn_info!(process_stdin_ref_stub, 1)); // ref
            set_field_with_stub(start + 8, lifecycle); // destroy
            let se = stdin_native_method(
                crate::fn_info!(process_stdin_set_encoding, 1; with_declared(1)),
                "setEncoding",
                1,
            );
            js_object_set_field(obj, start + 9, JSValue::from_bits(se.to_bits()));
            // field 22: Readable.read() returns buffered keyboard input.
            let read = stdin_native_method(
                crate::fn_info!(process_stdin_read, 1; with_declared(1)),
                "read",
                1,
            );
            js_object_set_field(obj, 22, JSValue::from_bits(read.to_bits()));
            let listeners = stdin_native_method(
                crate::fn_info!(process_stdin_listeners, 1; with_declared(1)),
                "listeners",
                1,
            );
            js_object_set_field(obj, 23, JSValue::from_bits(listeners.to_bits()));
        } else {
            set_field_with_stub(start + 7, lifecycle); // destroy
        }
    }
    obj
}
