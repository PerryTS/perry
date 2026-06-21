use crate::string::StringHeader;
use std::cell::RefCell;

/// Coerce a NaN-boxed JSValue to its display bytes, suitable for raw
/// stream writes. Used by `process.stdout.write` / `process.stderr.write`.
/// Mirrors Node's behavior: numbers/booleans/null/undefined coerce to
/// their string form; strings pass through verbatim.
fn jsvalue_to_write_bytes(value: f64) -> Vec<u8> {
    let s_ptr = crate::value::js_jsvalue_to_string(value);
    if s_ptr.is_null() {
        return Vec::new();
    }
    unsafe {
        let header = &*s_ptr;
        let len = header.byte_len as usize;
        let data = (s_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        std::slice::from_raw_parts(data, len).to_vec()
    }
}

/// `write` impl for process.stdout. Writes the value's display bytes to fd 1
/// without appending a newline, matching Node.js semantics.
extern "C" fn process_stdout_write_stub(
    _closure: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    use std::io::Write;
    let bytes = jsvalue_to_write_bytes(arg);
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    let _ = handle.write_all(&bytes);
    let _ = handle.flush();
    f64::from_bits(crate::value::TAG_TRUE)
}

/// `write` impl for process.stderr. Same as stdout, targeting fd 2.
extern "C" fn process_stderr_write_stub(
    _closure: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    use std::io::Write;
    let bytes = jsvalue_to_write_bytes(arg);
    let stderr = std::io::stderr();
    let mut handle = stderr.lock();
    let _ = handle.write_all(&bytes);
    let _ = handle.flush();
    f64::from_bits(crate::value::TAG_TRUE)
}

/// `write` impl for process.stdin. Reading from stdin via `.write` is
/// nonsensical; keep it as a no-op that returns `true`.
extern "C" fn process_stdin_write_noop_stub(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_TRUE)
}

extern "C" fn process_stream_emit_stub(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_TRUE)
}

extern "C" fn process_stream_on_once_stub(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// `setEncoding` impl for `process.stdin`. A Readable's `setEncoding(enc)`
/// returns the stream itself so callers can chain
/// (`process.stdin.setEncoding("utf8").on("data", …)`). The receiver is the
/// `IMPLICIT_THIS` bound by the method-dispatch path, so returning it mirrors
/// Node's `this`-returning contract. Encoding-aware reads remain future work.
extern "C" fn process_stream_set_encoding_stub(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    crate::object::js_implicit_this_get()
}

/// #3962: set when a TUI tears down stdin via `process.stdin.destroy()`,
/// `.pause()`, or `.unref()`. `perry-stdlib`'s readline `has_active` consults
/// `stdin_is_detached()` so the runtime stops holding the event loop open for
/// the stdin reader, letting the process quiesce after teardown without an
/// explicit `process.exit()`.
static STDIN_DETACHED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// True once `process.stdin` has been detached (`destroy`/`pause`/`unref`).
pub fn stdin_is_detached() -> bool {
    STDIN_DETACHED.load(std::sync::atomic::Ordering::Acquire)
}

/// `destroy`/`pause`/`unref` impl for `process.stdin` — releases the stdin
/// reader's hold on the event loop. No-op return (`undefined`).
extern "C" fn process_stdin_detach_stub(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    STDIN_DETACHED.store(true, std::sync::atomic::Ordering::Release);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

thread_local! {
    static STDIN_STREAM_SINGLETON: RefCell<usize> = const { RefCell::new(0) };
    static STDOUT_STREAM_SINGLETON: RefCell<usize> = const { RefCell::new(0) };
    static STDERR_STREAM_SINGLETON: RefCell<usize> = const { RefCell::new(0) };
}

fn string_key(key: &[u8]) -> *mut StringHeader {
    crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32)
}

fn set_stdin_bool_field(name: &[u8], value: bool) {
    STDIN_STREAM_SINGLETON.with(|slot| {
        let obj = *slot.borrow() as *mut crate::object::ObjectHeader;
        if obj.is_null() {
            return;
        }
        crate::object::js_object_set_field_by_name(
            obj,
            string_key(name),
            f64::from_bits(crate::value::JSValue::bool(value).bits()),
        );
    });
}

pub fn set_process_stdin_raw_state(enabled: bool) {
    set_stdin_bool_field(b"isRaw", enabled);
}

pub fn mark_process_stdin_destroyed() {
    set_stdin_bool_field(b"readable", false);
    set_stdin_bool_field(b"readableEnded", true);
    set_stdin_bool_field(b"destroyed", true);
    set_stdin_bool_field(b"closed", true);
    set_stdin_bool_field(b"isRaw", false);
}

pub fn scan_process_stream_singleton_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut visit_slot = |slot: &RefCell<usize>| {
        let mut value = slot.borrow_mut();
        if *value != 0 {
            let mut ptr = *value as *mut crate::object::ObjectHeader;
            if visitor.visit_raw_mut_ptr_slot(&mut ptr) {
                *value = ptr as usize;
            }
        }
    };
    STDIN_STREAM_SINGLETON.with(&mut visit_slot);
    STDOUT_STREAM_SINGLETON.with(&mut visit_slot);
    STDERR_STREAM_SINGLETON.with(&mut visit_slot);
}

/// Build a stream object with a `write` field bound to the given stub.
fn build_stream_object_with_write(
    write_stub: extern "C" fn(*const crate::closure::ClosureHeader, f64) -> f64,
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
            (
                if is_tty {
                    crate::tty::CLASS_ID_TTY_READ_STREAM
                } else {
                    0
                },
                keys,
                22,
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
    // #3962: install the appended listener-removal + lifecycle methods.
    // `on`/`once` above are no-ops here, so `addListener`/`removeListener`/
    // `off`/`removeAllListeners`/`resume` are no-ops too. On *stdin* (fd 0),
    // `pause`/`unref`/`destroy` additionally detach the reader so the loop can
    // quiesce after TUI teardown; on stdout/stderr they stay no-ops.
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
        set_field_with_stub(start, process_stream_on_once_stub); // addListener
        set_field_with_stub(start + 1, process_stream_on_once_stub); // removeListener
        set_field_with_stub(start + 2, process_stream_on_once_stub); // off
        set_field_with_stub(start + 3, process_stream_on_once_stub); // removeAllListeners
        set_field_with_stub(start + 4, lifecycle); // pause
        set_field_with_stub(start + 5, process_stream_on_once_stub); // resume
        set_field_with_stub(start + 6, lifecycle); // unref
        if is_stdin {
            set_field_with_stub(start + 7, process_stream_on_once_stub); // ref
            set_field_with_stub(start + 8, lifecycle); // destroy
            set_field_with_stub(start + 9, process_stream_set_encoding_stub); // setEncoding
        } else {
            set_field_with_stub(start + 7, lifecycle); // destroy
        }
    }
    obj
}

/// process.stdin -> stream object whose `.write(...)` is a no-op.
#[no_mangle]
pub extern "C" fn js_process_stdin() -> f64 {
    use crate::value::JSValue;
    let obj = STDIN_STREAM_SINGLETON.with(|slot| {
        let mut slot = slot.borrow_mut();
        if *slot == 0 {
            *slot = build_stream_object_with_write(
                process_stdin_write_noop_stub,
                0.0,
                f64::from_bits(crate::value::TAG_UNDEFINED),
            ) as usize;
        }
        *slot as *mut crate::object::ObjectHeader
    });
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// process.stdout -> stream object whose `.write(s)` writes `s` to fd 1.
#[no_mangle]
pub extern "C" fn js_process_stdout() -> f64 {
    use crate::value::JSValue;
    let obj = STDOUT_STREAM_SINGLETON.with(|slot| {
        let mut slot = slot.borrow_mut();
        if *slot == 0 {
            *slot = build_stream_object_with_write(
                process_stdout_write_stub,
                1.0,
                f64::from_bits(crate::value::TAG_TRUE),
            ) as usize;
        }
        *slot as *mut crate::object::ObjectHeader
    });
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// process.stderr -> stream object whose `.write(s)` writes `s` to fd 2.
#[no_mangle]
pub extern "C" fn js_process_stderr() -> f64 {
    use crate::value::JSValue;
    let obj = STDERR_STREAM_SINGLETON.with(|slot| {
        let mut slot = slot.borrow_mut();
        if *slot == 0 {
            *slot = build_stream_object_with_write(
                process_stderr_write_stub,
                2.0,
                f64::from_bits(crate::value::TAG_TRUE),
            ) as usize;
        }
        *slot as *mut crate::object::ObjectHeader
    });
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}
