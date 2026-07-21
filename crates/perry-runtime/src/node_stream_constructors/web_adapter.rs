//! node:stream — WHATWG Web-stream interop (`Readable.toWeb`/`fromWeb`, the
//! adapter pumps, and the fallback stub) split out of
//! node_stream_constructors.rs for the 2000-line file-size gate, #1987.
use super::super::*;
use super::*;
use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64, ClosureHeader,
};
use crate::object::{js_object_alloc, js_object_set_field};
use crate::value::JSValue;
use std::sync::atomic::{AtomicPtr, Ordering};

// ─────────────────────────────────────────────────────────────────
// #2521: Web-stream interop. Node exposes static helpers on the
// stream classes for converting between Node streams and WHATWG streams.
// The Web Streams implementation lives in perry-stdlib and registers the
// compact constructor/reader/writer callbacks below during stdlib init.
// Runtime class-specific helpers use those callbacks to bridge data between
// the two stream models; the historical generic functions remain as fallbacks
// for call sites where HIR did not preserve the stream class name.
// ─────────────────────────────────────────────────────────────────

type WebReadableNewFn = unsafe extern "C" fn(f64, f64, f64, f64) -> f64;
type WebReadableEnqueueFn = unsafe extern "C" fn(f64, f64) -> f64;
type WebReadableCloseFn = unsafe extern "C" fn(f64) -> f64;
type WebReadableErrorFn = unsafe extern "C" fn(f64, f64) -> f64;
type WebWritableNewFn = unsafe extern "C" fn(f64, f64, f64, f64, f64) -> f64;
type WebReadableGetReaderFn = unsafe extern "C" fn(f64) -> f64;
type WebReaderReadFn = unsafe extern "C" fn(f64) -> *mut crate::promise::Promise;
type WebWritableGetWriterFn = unsafe extern "C" fn(f64) -> f64;
type WebWriterWriteFn = unsafe extern "C" fn(f64, f64) -> *mut crate::promise::Promise;
type WebWriterCloseFn = unsafe extern "C" fn(f64) -> *mut crate::promise::Promise;
type WebWriterAbortFn = unsafe extern "C" fn(f64, f64) -> *mut crate::promise::Promise;

static WEB_READABLE_NEW_PTR: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static WEB_READABLE_ENQUEUE_PTR: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static WEB_READABLE_CLOSE_PTR: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static WEB_READABLE_ERROR_PTR: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static WEB_WRITABLE_NEW_PTR: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static WEB_READABLE_GET_READER_PTR: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static WEB_READER_READ_PTR: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static WEB_WRITABLE_GET_WRITER_PTR: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static WEB_WRITER_WRITE_PTR: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static WEB_WRITER_CLOSE_PTR: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static WEB_WRITER_ABORT_PTR: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

#[no_mangle]
pub unsafe extern "C" fn js_register_node_stream_web_adapter_callbacks(
    readable_new: WebReadableNewFn,
    readable_enqueue: WebReadableEnqueueFn,
    readable_close: WebReadableCloseFn,
    readable_error: WebReadableErrorFn,
    writable_new: WebWritableNewFn,
    readable_get_reader: WebReadableGetReaderFn,
    reader_read: WebReaderReadFn,
    writable_get_writer: WebWritableGetWriterFn,
    writer_write: WebWriterWriteFn,
    writer_close: WebWriterCloseFn,
    writer_abort: WebWriterAbortFn,
) {
    WEB_READABLE_NEW_PTR.store(readable_new as *mut (), Ordering::Release);
    WEB_READABLE_ENQUEUE_PTR.store(readable_enqueue as *mut (), Ordering::Release);
    WEB_READABLE_CLOSE_PTR.store(readable_close as *mut (), Ordering::Release);
    WEB_READABLE_ERROR_PTR.store(readable_error as *mut (), Ordering::Release);
    WEB_WRITABLE_NEW_PTR.store(writable_new as *mut (), Ordering::Release);
    WEB_READABLE_GET_READER_PTR.store(readable_get_reader as *mut (), Ordering::Release);
    WEB_READER_READ_PTR.store(reader_read as *mut (), Ordering::Release);
    WEB_WRITABLE_GET_WRITER_PTR.store(writable_get_writer as *mut (), Ordering::Release);
    WEB_WRITER_WRITE_PTR.store(writer_write as *mut (), Ordering::Release);
    WEB_WRITER_CLOSE_PTR.store(writer_close as *mut (), Ordering::Release);
    WEB_WRITER_ABORT_PTR.store(writer_abort as *mut (), Ordering::Release);
}

macro_rules! load_web_callback {
    ($slot:expr, $ty:ty) => {{
        let p = $slot.load(Ordering::Acquire);
        if p.is_null() {
            None
        } else {
            Some(unsafe { std::mem::transmute::<*mut (), $ty>(p) })
        }
    }};
}

fn web_readable_new() -> Option<WebReadableNewFn> {
    load_web_callback!(WEB_READABLE_NEW_PTR, WebReadableNewFn)
}

fn web_readable_enqueue() -> Option<WebReadableEnqueueFn> {
    load_web_callback!(WEB_READABLE_ENQUEUE_PTR, WebReadableEnqueueFn)
}

fn web_readable_close() -> Option<WebReadableCloseFn> {
    load_web_callback!(WEB_READABLE_CLOSE_PTR, WebReadableCloseFn)
}

fn web_readable_error() -> Option<WebReadableErrorFn> {
    load_web_callback!(WEB_READABLE_ERROR_PTR, WebReadableErrorFn)
}

fn web_writable_new() -> Option<WebWritableNewFn> {
    load_web_callback!(WEB_WRITABLE_NEW_PTR, WebWritableNewFn)
}

fn web_readable_get_reader() -> Option<WebReadableGetReaderFn> {
    load_web_callback!(WEB_READABLE_GET_READER_PTR, WebReadableGetReaderFn)
}

fn web_reader_read() -> Option<WebReaderReadFn> {
    load_web_callback!(WEB_READER_READ_PTR, WebReaderReadFn)
}

fn web_writable_get_writer() -> Option<WebWritableGetWriterFn> {
    load_web_callback!(WEB_WRITABLE_GET_WRITER_PTR, WebWritableGetWriterFn)
}

fn web_writer_write() -> Option<WebWriterWriteFn> {
    load_web_callback!(WEB_WRITER_WRITE_PTR, WebWriterWriteFn)
}

fn web_writer_close() -> Option<WebWriterCloseFn> {
    load_web_callback!(WEB_WRITER_CLOSE_PTR, WebWriterCloseFn)
}

fn web_writer_abort() -> Option<WebWriterAbortFn> {
    load_web_callback!(WEB_WRITER_ABORT_PTR, WebWriterAbortFn)
}

fn closure_value(closure: *mut ClosureHeader) -> f64 {
    f64::from_bits(JSValue::pointer(closure as *const u8).bits())
}

fn closure_with_stream(func: *const u8, node_stream: f64) -> f64 {
    let closure = js_closure_alloc(func, 1);
    js_closure_set_capture_f64(closure, 0, node_stream);
    closure_value(closure)
}

fn build_enumerable_object(fields: &[(&[u8], f64)]) -> f64 {
    let obj = js_object_alloc(0, fields.len() as u32);
    let mut keys = crate::array::js_array_alloc(fields.len() as u32);
    for (idx, (name, value)) in fields.iter().enumerate() {
        keys = crate::array::js_array_push_f64(keys, string_value(name));
        js_object_set_field(obj, idx as u32, JSValue::from_bits(value.to_bits()));
    }
    crate::object::js_object_set_keys(obj, keys);
    box_pointer(obj as *const u8)
}

fn build_web_read_result(value: f64, done: bool) -> f64 {
    build_enumerable_object(&[(b"value", value), (b"done", bool_value(done))])
}

fn property_value(value: f64, name: &[u8]) -> f64 {
    unsafe { crate::value::js_get_property(value, name.as_ptr() as i64, name.len() as i64) }
}

fn call_stream_callback(callback: f64, err: f64) {
    if !is_callable_value(callback) {
        return;
    }
    let arg = if err.to_bits() == TAG_UNDEFINED {
        f64::from_bits(TAG_NULL)
    } else {
        err
    };
    unsafe {
        let _ = crate::closure::js_native_call_value(callback, [arg].as_ptr(), 1);
    }
}

extern "C" fn node_to_web_readable_pull(closure: *const ClosureHeader, controller: f64) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let node_stream = js_closure_get_capture_f64(closure, 0);
    let chunk = read_stream_with_size_arg(node_stream, f64::from_bits(TAG_UNDEFINED));
    match chunk.to_bits() {
        TAG_NULL | TAG_UNDEFINED => {
            if stream_hidden_ended(node_stream) || !readable_chunks_nonempty(node_stream) {
                if let Some(close) = web_readable_close() {
                    unsafe {
                        close(controller);
                    }
                }
            }
        }
        _ => {
            if let Some(enqueue) = web_readable_enqueue() {
                unsafe {
                    enqueue(controller, chunk);
                }
            }
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn node_to_web_readable_cancel(closure: *const ClosureHeader, reason: f64) -> f64 {
    if !closure.is_null() {
        destroy_stream(js_closure_get_capture_f64(closure, 0), reason);
    }
    f64::from_bits(TAG_UNDEFINED)
}

fn node_readable_to_web(node_stream: f64) -> Option<f64> {
    let readable_new = web_readable_new()?;
    crate::closure::js_register_closure_arity(node_to_web_readable_pull as *const u8, 1);
    crate::closure::js_register_closure_arity(node_to_web_readable_cancel as *const u8, 1);
    let pull = js_closure_alloc(node_to_web_readable_pull as *const u8, 1);
    js_closure_set_capture_f64(pull, 0, node_stream);
    let cancel = js_closure_alloc(node_to_web_readable_cancel as *const u8, 1);
    js_closure_set_capture_f64(cancel, 0, node_stream);
    Some(unsafe {
        readable_new(
            f64::from_bits(TAG_UNDEFINED),
            closure_value(pull),
            closure_value(cancel),
            1.0,
        )
    })
}

extern "C" fn fallback_web_reader_read(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return resolved_promise(build_web_read_result(f64::from_bits(TAG_UNDEFINED), true));
    }
    let node_stream = js_closure_get_capture_f64(closure, 0);
    let chunk = read_stream_with_size_arg(node_stream, f64::from_bits(TAG_UNDEFINED));
    let result = match chunk.to_bits() {
        TAG_NULL | TAG_UNDEFINED => {
            let done = stream_hidden_ended(node_stream) || !readable_chunks_nonempty(node_stream);
            build_web_read_result(f64::from_bits(TAG_UNDEFINED), done)
        }
        _ => build_web_read_result(chunk, false),
    };
    resolved_promise(result)
}

extern "C" fn fallback_web_reader_cancel(closure: *const ClosureHeader, reason: f64) -> f64 {
    if !closure.is_null() {
        destroy_stream(js_closure_get_capture_f64(closure, 0), reason);
    }
    resolved_promise(f64::from_bits(TAG_UNDEFINED))
}

extern "C" fn fallback_web_readable_get_reader(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let node_stream = js_closure_get_capture_f64(closure, 0);
    crate::closure::js_register_closure_arity(fallback_web_reader_read as *const u8, 0);
    crate::closure::js_register_closure_arity(fallback_web_reader_cancel as *const u8, 1);
    build_enumerable_object(&[
        (
            b"read",
            closure_with_stream(fallback_web_reader_read as *const u8, node_stream),
        ),
        (
            b"cancel",
            closure_with_stream(fallback_web_reader_cancel as *const u8, node_stream),
        ),
    ])
}

fn fallback_node_readable_to_web(node_stream: f64) -> f64 {
    crate::closure::js_register_closure_arity(fallback_web_readable_get_reader as *const u8, 0);
    crate::closure::js_register_closure_arity(fallback_web_reader_cancel as *const u8, 1);
    build_enumerable_object(&[
        (
            b"getReader",
            closure_with_stream(fallback_web_readable_get_reader as *const u8, node_stream),
        ),
        (
            b"cancel",
            closure_with_stream(fallback_web_reader_cancel as *const u8, node_stream),
        ),
    ])
}

extern "C" fn node_to_web_writable_write(closure: *const ClosureHeader, chunk: f64) -> f64 {
    if !closure.is_null() {
        let node_stream = js_closure_get_capture_f64(closure, 0);
        let _ = write_writable_chunk(
            node_stream,
            chunk,
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
        );
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn node_to_web_writable_close(closure: *const ClosureHeader) -> f64 {
    if !closure.is_null() {
        let node_stream = js_closure_get_capture_f64(closure, 0);
        finish_stream_with_args(
            node_stream,
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
        );
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn node_to_web_writable_abort(closure: *const ClosureHeader, reason: f64) -> f64 {
    if !closure.is_null() {
        destroy_stream(js_closure_get_capture_f64(closure, 0), reason);
    }
    f64::from_bits(TAG_UNDEFINED)
}

fn node_writable_to_web(node_stream: f64) -> Option<f64> {
    let writable_new = web_writable_new()?;
    crate::closure::js_register_closure_arity(node_to_web_writable_write as *const u8, 1);
    crate::closure::js_register_closure_arity(node_to_web_writable_close as *const u8, 0);
    crate::closure::js_register_closure_arity(node_to_web_writable_abort as *const u8, 1);
    let write = js_closure_alloc(node_to_web_writable_write as *const u8, 1);
    js_closure_set_capture_f64(write, 0, node_stream);
    let close = js_closure_alloc(node_to_web_writable_close as *const u8, 1);
    js_closure_set_capture_f64(close, 0, node_stream);
    let abort = js_closure_alloc(node_to_web_writable_abort as *const u8, 1);
    js_closure_set_capture_f64(abort, 0, node_stream);
    Some(unsafe {
        writable_new(
            f64::from_bits(TAG_UNDEFINED),
            closure_value(write),
            closure_value(close),
            closure_value(abort),
            1.0,
        )
    })
}

extern "C" fn fallback_web_writer_write(closure: *const ClosureHeader, chunk: f64) -> f64 {
    if !closure.is_null() {
        let node_stream = js_closure_get_capture_f64(closure, 0);
        let _ = write_writable_chunk(
            node_stream,
            chunk,
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
        );
    }
    resolved_promise(f64::from_bits(TAG_UNDEFINED))
}

extern "C" fn fallback_web_writer_close(closure: *const ClosureHeader) -> f64 {
    if !closure.is_null() {
        finish_stream_with_args(
            js_closure_get_capture_f64(closure, 0),
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
        );
    }
    resolved_promise(f64::from_bits(TAG_UNDEFINED))
}

extern "C" fn fallback_web_writer_abort(closure: *const ClosureHeader, reason: f64) -> f64 {
    if !closure.is_null() {
        destroy_stream(js_closure_get_capture_f64(closure, 0), reason);
    }
    resolved_promise(f64::from_bits(TAG_UNDEFINED))
}

extern "C" fn fallback_web_writable_get_writer(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let node_stream = js_closure_get_capture_f64(closure, 0);
    crate::closure::js_register_closure_arity(fallback_web_writer_write as *const u8, 1);
    crate::closure::js_register_closure_arity(fallback_web_writer_close as *const u8, 0);
    crate::closure::js_register_closure_arity(fallback_web_writer_abort as *const u8, 1);
    build_enumerable_object(&[
        (
            b"write",
            closure_with_stream(fallback_web_writer_write as *const u8, node_stream),
        ),
        (
            b"close",
            closure_with_stream(fallback_web_writer_close as *const u8, node_stream),
        ),
        (
            b"abort",
            closure_with_stream(fallback_web_writer_abort as *const u8, node_stream),
        ),
    ])
}

fn fallback_node_writable_to_web(node_stream: f64) -> f64 {
    crate::closure::js_register_closure_arity(fallback_web_writable_get_writer as *const u8, 0);
    crate::closure::js_register_closure_arity(fallback_web_writer_abort as *const u8, 1);
    build_enumerable_object(&[
        (
            b"getWriter",
            closure_with_stream(fallback_web_writable_get_writer as *const u8, node_stream),
        ),
        (
            b"abort",
            closure_with_stream(fallback_web_writer_abort as *const u8, node_stream),
        ),
    ])
}

pub(crate) fn js_node_stream_readable_to_web_method_value(node_stream: f64) -> f64 {
    fallback_node_readable_to_web(node_stream)
}

pub(crate) fn js_node_stream_writable_to_web_method_value(node_stream: f64) -> f64 {
    fallback_node_writable_to_web(node_stream)
}

pub(crate) fn js_node_stream_duplex_to_web_method_value(node_stream: f64) -> f64 {
    web_pair_object(
        fallback_node_readable_to_web(node_stream),
        fallback_node_writable_to_web(node_stream),
    )
}

fn web_pair_object(readable: f64, writable: f64) -> f64 {
    build_enumerable_object(&[(b"readable", readable), (b"writable", writable)])
}

fn install_web_readable_adapter(node_stream: f64, web_stream: f64) -> bool {
    let Some(get_reader) = web_readable_get_reader() else {
        return false;
    };
    let reader = unsafe { get_reader(web_stream) };
    if reader.to_bits() == TAG_UNDEFINED {
        return false;
    }
    crate::closure::js_register_closure_arity(web_to_node_readable_read as *const u8, 1);
    let read = js_closure_alloc(web_to_node_readable_read as *const u8, 2);
    js_closure_set_capture_f64(read, 0, node_stream);
    js_closure_set_capture_f64(read, 1, reader);
    set_hidden_value(node_stream, hidden_read_key(), closure_value(read));
    set_hidden_value(
        node_stream,
        hidden_default_read_error_key(),
        f64::from_bits(TAG_FALSE),
    );
    true
}

extern "C" fn web_to_node_readable_read(closure: *const ClosureHeader, _size: f64) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let node_stream = js_closure_get_capture_f64(closure, 0);
    let reader = js_closure_get_capture_f64(closure, 1);
    if has_truthy_hidden(node_stream, hidden_key(b"webReadablePumping")) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    set_hidden_value(
        node_stream,
        hidden_key(b"webReadablePumping"),
        f64::from_bits(TAG_TRUE),
    );
    pump_web_reader(node_stream, reader);
    f64::from_bits(TAG_UNDEFINED)
}

fn pump_web_reader(node_stream: f64, reader: f64) {
    if stream_destroyed(node_stream) || stream_hidden_ended(node_stream) {
        return;
    }
    let Some(read) = web_reader_read() else {
        return;
    };
    let promise = unsafe { read(reader) };
    if promise.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(web_to_node_readable_read_fulfilled as *const u8, 1);
    crate::closure::js_register_closure_arity(web_to_node_readable_read_rejected as *const u8, 1);
    let fulfilled = js_closure_alloc(web_to_node_readable_read_fulfilled as *const u8, 2);
    js_closure_set_capture_f64(fulfilled, 0, node_stream);
    js_closure_set_capture_f64(fulfilled, 1, reader);
    let rejected = js_closure_alloc(web_to_node_readable_read_rejected as *const u8, 1);
    js_closure_set_capture_f64(rejected, 0, node_stream);
    crate::promise::js_promise_attach_handlers(promise, fulfilled, rejected);
}

extern "C" fn web_to_node_readable_read_fulfilled(
    closure: *const ClosureHeader,
    result: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let node_stream = js_closure_get_capture_f64(closure, 0);
    let reader = js_closure_get_capture_f64(closure, 1);
    let done = property_value(result, b"done");
    if crate::value::js_is_truthy(done) != 0 {
        set_hidden_value(
            node_stream,
            hidden_key(b"webReadablePumping"),
            f64::from_bits(TAG_FALSE),
        );
        let _ = push_chunk(node_stream, f64::from_bits(TAG_NULL));
        return f64::from_bits(TAG_UNDEFINED);
    }
    let value = property_value(result, b"value");
    let _ = push_chunk(node_stream, value);
    pump_web_reader(node_stream, reader);
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn web_to_node_readable_read_rejected(
    closure: *const ClosureHeader,
    reason: f64,
) -> f64 {
    if !closure.is_null() {
        let node_stream = js_closure_get_capture_f64(closure, 0);
        set_hidden_value(
            node_stream,
            hidden_key(b"webReadablePumping"),
            f64::from_bits(TAG_FALSE),
        );
        destroy_stream(node_stream, reason);
    }
    f64::from_bits(TAG_UNDEFINED)
}

fn install_web_writable_adapter(node_stream: f64, web_stream: f64) -> bool {
    let Some(get_writer) = web_writable_get_writer() else {
        return false;
    };
    let writer = unsafe { get_writer(web_stream) };
    if writer.to_bits() == TAG_UNDEFINED {
        return false;
    }
    crate::closure::js_register_closure_arity(web_to_node_writable_write as *const u8, 3);
    crate::closure::js_register_closure_arity(web_to_node_writable_final as *const u8, 1);
    crate::closure::js_register_closure_arity(web_to_node_writable_destroy as *const u8, 2);
    let write = js_closure_alloc(web_to_node_writable_write as *const u8, 1);
    js_closure_set_capture_f64(write, 0, writer);
    let final_cb = js_closure_alloc(web_to_node_writable_final as *const u8, 1);
    js_closure_set_capture_f64(final_cb, 0, writer);
    let destroy = js_closure_alloc(web_to_node_writable_destroy as *const u8, 1);
    js_closure_set_capture_f64(destroy, 0, writer);
    set_hidden_value(node_stream, hidden_write_key(), closure_value(write));
    set_hidden_value(
        node_stream,
        hidden_writable_final_key(),
        closure_value(final_cb),
    );
    set_hidden_value(
        node_stream,
        hidden_writable_final_invoked_key(),
        f64::from_bits(TAG_FALSE),
    );
    set_hidden_value(
        node_stream,
        hidden_writable_final_pending_key(),
        f64::from_bits(TAG_FALSE),
    );
    set_hidden_value(
        node_stream,
        hidden_key(STREAM_DESTROY_KEY),
        closure_value(destroy),
    );
    true
}

extern "C" fn web_to_node_writable_write(
    closure: *const ClosureHeader,
    chunk: f64,
    _encoding: f64,
    callback: f64,
) -> f64 {
    if closure.is_null() {
        call_stream_callback(callback, f64::from_bits(TAG_UNDEFINED));
        return f64::from_bits(TAG_UNDEFINED);
    }
    let writer = js_closure_get_capture_f64(closure, 0);
    if let Some(write) = web_writer_write() {
        let promise = unsafe { write(writer, chunk) };
        attach_web_writable_callback(promise, callback);
    } else {
        call_stream_callback(callback, f64::from_bits(TAG_UNDEFINED));
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn web_to_node_writable_final(closure: *const ClosureHeader, callback: f64) -> f64 {
    if closure.is_null() {
        call_stream_callback(callback, f64::from_bits(TAG_UNDEFINED));
        return f64::from_bits(TAG_UNDEFINED);
    }
    let writer = js_closure_get_capture_f64(closure, 0);
    if let Some(close) = web_writer_close() {
        let promise = unsafe { close(writer) };
        attach_web_writable_callback(promise, callback);
    } else {
        call_stream_callback(callback, f64::from_bits(TAG_UNDEFINED));
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn web_to_node_writable_destroy(
    closure: *const ClosureHeader,
    err: f64,
    callback: f64,
) -> f64 {
    if closure.is_null() {
        call_stream_callback(callback, f64::from_bits(TAG_UNDEFINED));
        return f64::from_bits(TAG_UNDEFINED);
    }
    let writer = js_closure_get_capture_f64(closure, 0);
    if let Some(abort) = web_writer_abort() {
        let reason = if err.to_bits() == TAG_NULL {
            f64::from_bits(TAG_UNDEFINED)
        } else {
            err
        };
        let promise = unsafe { abort(writer, reason) };
        attach_web_writable_callback(promise, callback);
    } else {
        call_stream_callback(callback, f64::from_bits(TAG_UNDEFINED));
    }
    f64::from_bits(TAG_UNDEFINED)
}

fn attach_web_writable_callback(promise: *mut crate::promise::Promise, callback: f64) {
    if promise.is_null() {
        call_stream_callback(callback, f64::from_bits(TAG_UNDEFINED));
        return;
    }
    crate::closure::js_register_closure_arity(web_to_node_writable_fulfilled as *const u8, 1);
    crate::closure::js_register_closure_arity(web_to_node_writable_rejected as *const u8, 1);
    let fulfilled = js_closure_alloc(web_to_node_writable_fulfilled as *const u8, 1);
    js_closure_set_capture_f64(fulfilled, 0, callback);
    let rejected = js_closure_alloc(web_to_node_writable_rejected as *const u8, 1);
    js_closure_set_capture_f64(rejected, 0, callback);
    crate::promise::js_promise_attach_handlers(promise, fulfilled, rejected);
}

extern "C" fn web_to_node_writable_fulfilled(closure: *const ClosureHeader, _value: f64) -> f64 {
    if !closure.is_null() {
        call_stream_callback(
            js_closure_get_capture_f64(closure, 0),
            f64::from_bits(TAG_UNDEFINED),
        );
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn web_to_node_writable_rejected(closure: *const ClosureHeader, reason: f64) -> f64 {
    if !closure.is_null() {
        call_stream_callback(js_closure_get_capture_f64(closure, 0), reason);
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_node_stream_readable_to_web(node_stream: f64) -> f64 {
    node_readable_to_web(node_stream).unwrap_or_else(|| fallback_node_readable_to_web(node_stream))
}

#[no_mangle]
pub extern "C" fn js_node_stream_writable_to_web(node_stream: f64) -> f64 {
    node_writable_to_web(node_stream).unwrap_or_else(|| fallback_node_writable_to_web(node_stream))
}

#[no_mangle]
pub extern "C" fn js_node_stream_duplex_to_web(node_stream: f64) -> f64 {
    match (
        node_readable_to_web(node_stream),
        node_writable_to_web(node_stream),
    ) {
        (Some(readable), Some(writable)) => web_pair_object(readable, writable),
        _ => web_pair_object(
            fallback_node_readable_to_web(node_stream),
            fallback_node_writable_to_web(node_stream),
        ),
    }
}

#[no_mangle]
pub extern "C" fn js_node_stream_readable_from_web(web_stream: f64, opts: f64) -> f64 {
    let readable = js_node_stream_readable_new(readable_from_options(opts));
    if install_web_readable_adapter(readable, web_stream) {
        readable
    } else {
        js_node_stream_from_web(web_stream)
    }
}

#[no_mangle]
pub extern "C" fn js_node_stream_writable_from_web(web_stream: f64, opts: f64) -> f64 {
    let writable = js_node_stream_writable_new(opts);
    if install_web_writable_adapter(writable, web_stream) {
        writable
    } else {
        js_node_stream_from_web(web_stream)
    }
}

#[no_mangle]
pub extern "C" fn js_node_stream_duplex_from_web(pair: f64, opts: f64) -> f64 {
    let readable_web = property_value(pair, b"readable");
    let writable_web = property_value(pair, b"writable");
    let duplex = js_node_stream_duplex_new(opts);
    let readable_ok = readable_web.to_bits() != TAG_UNDEFINED
        && install_web_readable_adapter(duplex, readable_web);
    let writable_ok = writable_web.to_bits() != TAG_UNDEFINED
        && install_web_writable_adapter(duplex, writable_web);
    if writable_ok {
        set_hidden_value(
            duplex,
            hidden_key(b"writableCustomSink"),
            f64::from_bits(TAG_TRUE),
        );
    }
    if readable_ok || writable_ok {
        duplex
    } else {
        js_node_stream_from_web(pair)
    }
}

/// A WHATWG-stream-shaped stub: an object carrying both `getReader` and
/// `getWriter` method stubs. A real `ReadableStream` only has `getReader`
/// and a `WritableStream` only `getWriter`, but the single `js_node_stream_to_web`
/// entry can't tell which class `.toWeb` was called on (the NativeMethodCall
/// drops the class), so the union shape lets `Readable.toWeb`,
/// `Writable.toWeb`, and the `{ readable, writable }` pair from
/// `Duplex.toWeb` all satisfy their `typeof x.getReader/getWriter === "function"`
/// existence checks. Data isn't forwarded between the Node and WHATWG
/// universes — that's the remaining #1540 gap.
pub(super) fn build_web_stream_stub() -> f64 {
    let methods: [(&str, StubFn); 2] = [
        ("getReader", cast0(ns_undefined0)),
        ("getWriter", cast0(ns_undefined0)),
    ];
    let obj = build_object(&methods, WEB_STREAM_SHAPE_ID + methods.len() as u32);
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// `Readable.toWeb` / `Writable.toWeb` / `Duplex.toWeb` — returns a
/// web-stream-shaped stub (#1540). For Duplex the result also exposes
/// `readable` / `writable` web-stream stubs so `pair.readable.getReader`
/// / `pair.writable.getWriter` resolve.
#[no_mangle]
pub extern "C" fn js_node_stream_to_web(node_stream: f64) -> f64 {
    let readable = get_hidden_value(node_stream, hidden_readable_flag_key()).is_some();
    let writable = get_hidden_value(node_stream, hidden_writable_flag_key()).is_some();
    match (readable, writable) {
        (true, true) => return js_node_stream_duplex_to_web(node_stream),
        (true, false) => return js_node_stream_readable_to_web(node_stream),
        (false, true) => return js_node_stream_writable_to_web(node_stream),
        (false, false) => {}
    }

    let top = build_web_stream_stub();
    set_hidden_value(top, hidden_key(b"readable"), build_web_stream_stub());
    set_hidden_value(top, hidden_key(b"writable"), build_web_stream_stub());
    top
}

/// Generic `.fromWeb` fallback used when the lowering cannot preserve the
/// static stream class. Prefer real adapters when the input shape makes a
/// direction clear, then fall back to the legacy Duplex stub.
#[no_mangle]
pub extern "C" fn js_node_stream_from_web(web_stream: f64) -> f64 {
    let readable_web = property_value(web_stream, b"readable");
    let writable_web = property_value(web_stream, b"writable");
    if readable_web.to_bits() != TAG_UNDEFINED || writable_web.to_bits() != TAG_UNDEFINED {
        return js_node_stream_duplex_from_web(web_stream, f64::from_bits(TAG_UNDEFINED));
    }

    let readable = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    if install_web_readable_adapter(readable, web_stream) {
        return readable;
    }

    let writable = js_node_stream_writable_new(f64::from_bits(TAG_UNDEFINED));
    if install_web_writable_adapter(writable, web_stream) {
        return writable;
    }

    js_node_stream_duplex_new(f64::from_bits(TAG_UNDEFINED))
}
