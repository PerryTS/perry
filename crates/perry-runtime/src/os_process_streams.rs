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

thread_local! {
    static STDIN_STREAM_SINGLETON: RefCell<usize> = const { RefCell::new(0) };
    static STDOUT_STREAM_SINGLETON: RefCell<usize> = const { RefCell::new(0) };
    static STDERR_STREAM_SINGLETON: RefCell<usize> = const { RefCell::new(0) };
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

    let (class_id, packed, field_count) = if is_tty && fd_i == 0 {
        (
            crate::tty::CLASS_ID_TTY_READ_STREAM,
            b"write\0fd\0emit\0on\0once\0writable\0isRaw\0isTTY\0".as_slice(),
            8,
        )
    } else if is_tty {
        (
            crate::tty::CLASS_ID_TTY_WRITE_STREAM,
            b"write\0fd\0emit\0on\0once\0writable\0addListener\0removeListener\0off\0removeAllListeners\0".as_slice(),
            10,
        )
    } else {
        (0, b"write\0fd\0emit\0on\0once\0writable\0".as_slice(), 6)
    };
    let obj = if class_id == 0 {
        js_object_alloc_with_shape(
            0x7FFF_FF22,
            field_count,
            packed.as_ptr(),
            packed.len() as u32,
        )
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
    if is_tty && fd_i == 0 {
        js_object_set_field(obj, 6, JSValue::from_bits(crate::value::TAG_FALSE));
        js_object_set_field(obj, 7, JSValue::from_bits(crate::value::TAG_TRUE));
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
