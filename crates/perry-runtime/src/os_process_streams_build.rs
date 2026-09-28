//! `build_stream_object_with_write`: the shape/field builder behind the
//! `process.stdin` / `process.stdout` / `process.stderr` stream singletons.
//! Split out of `os_process_streams.rs` to stay under the 2,000-line cap
//! (#10750).

use super::*;

/// Build a stream object with a `write` field bound to the given stub.
pub(super) fn build_stream_object_with_write(
    write_stub: extern "C" fn(*const crate::closure::ClosureHeader, f64, f64, f64) -> f64,
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
                b"write\0fd\0emit\0on\0once\0writable\0addListener\0removeListener\0off\0removeAllListeners\0".to_vec(),
                10,
                None,
            )
        } else {
            let mut keys = b"write\0fd\0emit\0on\0once\0writable\0".to_vec();
            keys.extend_from_slice(GENERIC_TEARDOWN_KEYS);
            (0, keys, 14, Some(6))
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
    // callback])`. Register its arity so dispatch pads/truncates to exactly the
    // three the stub declares (Direct dispatch would otherwise size the call to
    // the call site, dropping the trailing callback — #6672).
    crate::closure::js_register_closure_arity(write_stub as *const u8, 3);
    let closure = js_closure_alloc(write_stub as *const u8, 0);
    let cval = JSValue::pointer(closure as *const u8);
    js_object_set_field(obj, 0, cval);
    js_object_set_field(obj, 1, JSValue::number(fd));
    let emit = js_closure_alloc(process_stream_emit_stub as *const u8, 0);
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
        let on = stdin_native_method(process_stdin_on as *const u8, "on", 2);
        js_object_set_field(obj, 3, JSValue::from_bits(on.to_bits()));
        // `once` routes through the same registry as `on`/`addListener` so a
        // one-shot listener registered on an aliased binding is not dropped either.
        let once = stdin_native_method(process_stdin_add_listener_once as *const u8, "once", 2);
        js_object_set_field(obj, 4, JSValue::from_bits(once.to_bits()));
    } else {
        let on = js_closure_alloc(process_stream_on_once_stub as *const u8, 0);
        js_object_set_field(obj, 3, JSValue::pointer(on as *const u8));
        let once = js_closure_alloc(process_stream_on_once_stub as *const u8, 0);
        js_object_set_field(obj, 4, JSValue::pointer(once as *const u8));
    }
    js_object_set_field(obj, 5, JSValue::from_bits(writable.to_bits()));
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
        let set_field_with_stub =
            |idx: u32, stub: extern "C" fn(*const crate::closure::ClosureHeader, f64) -> f64| {
                let c = js_closure_alloc(stub as *const u8, 0);
                js_object_set_field(obj, idx, JSValue::pointer(c as *const u8));
            };
        let lifecycle: extern "C" fn(*const crate::closure::ClosureHeader, f64) -> f64 = if is_stdin
        {
            process_stdin_detach_stub
        } else {
            process_stream_on_once_stub
        };
        // On stdin these must be REAL: a TUI registers its keyboard through an
        // aliased binding (`stdin.addListener("readable", handler)`), which lands
        // here rather than on codegen's direct `process.stdin.on(...)` extern. As
        // no-op stubs they silently discarded the handler.
        if is_stdin {
            let add =
                stdin_native_method(process_stdin_add_listener as *const u8, "addListener", 2);
            js_object_set_field(obj, start, JSValue::from_bits(add.to_bits()));
            let rm = stdin_native_method(
                process_stdin_remove_listener as *const u8,
                "removeListener",
                2,
            );
            js_object_set_field(obj, start + 1, JSValue::from_bits(rm.to_bits()));
            let off = stdin_native_method(process_stdin_remove_listener as *const u8, "off", 2);
            js_object_set_field(obj, start + 2, JSValue::from_bits(off.to_bits()));
        } else {
            set_field_with_stub(start, process_stream_on_once_stub); // addListener
            set_field_with_stub(start + 1, process_stream_on_once_stub); // removeListener
            set_field_with_stub(start + 2, process_stream_on_once_stub); // off
        }
        if is_stdin {
            let remove_all = stdin_native_method(
                process_stdin_remove_all_listeners as *const u8,
                "removeAllListeners",
                1,
            );
            js_object_set_field(obj, start + 3, JSValue::from_bits(remove_all.to_bits()));
        } else {
            set_field_with_stub(start + 3, process_stream_on_once_stub);
        }
        set_field_with_stub(start + 4, lifecycle); // pause
                                                   // resume: real flowing-mode start on stdin, no-op on stdout/stderr.
        set_field_with_stub(
            start + 5,
            if is_stdin {
                process_stdin_resume
            } else {
                process_stream_on_once_stub
            },
        ); // resume
           // #9676: on stdin, `unref`/`ref` are a SYMMETRIC pair that only moves
           // the event-loop hold; on stdout/stderr `unref` stays the shared no-op.
        set_field_with_stub(
            start + 6,
            if is_stdin {
                process_stdin_unref_stub
            } else {
                process_stream_on_once_stub
            },
        ); // unref
        if is_stdin {
            set_field_with_stub(start + 7, process_stdin_ref_stub); // ref
            set_field_with_stub(start + 8, lifecycle); // destroy
            if is_stdin {
                let se =
                    stdin_native_method(process_stdin_set_encoding as *const u8, "setEncoding", 1);
                js_object_set_field(obj, start + 9, JSValue::from_bits(se.to_bits()));
            } else {
                set_field_with_stub(start + 9, process_stream_set_encoding_stub);
                // setEncoding
            }
            // field 22: Readable.read() returns buffered keyboard input.
            let read = stdin_native_method(process_stdin_read as *const u8, "read", 1);
            js_object_set_field(obj, 22, JSValue::from_bits(read.to_bits()));
            let listeners =
                stdin_native_method(process_stdin_listeners as *const u8, "listeners", 1);
            js_object_set_field(obj, 23, JSValue::from_bits(listeners.to_bits()));
        } else {
            set_field_with_stub(start + 7, lifecycle); // destroy
        }
    }
    obj
}
