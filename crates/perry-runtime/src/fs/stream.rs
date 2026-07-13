//! createReadStream / createWriteStream — real-file-backed streams.

use super::*;

use std::cell::RefCell;
use std::collections::HashMap as StdHashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::closure::{
    js_closure_alloc, js_closure_get_capture_ptr, js_closure_set_capture_ptr, ClosureHeader,
};
use crate::object::{js_object_set_field, ObjectHeader};
use crate::value::JSValue;

const READ_STREAM_DEFAULT_HWM: usize = 64 * 1024;
const WRITE_STREAM_DEFAULT_HWM: usize = 16 * 1024;

#[derive(Clone, Copy, Eq, PartialEq)]
enum StreamKind {
    Read,
    Write,
}

#[derive(Clone, Copy)]
enum FdOwner {
    Path,
    External,
    FileHandle(f64),
}

#[derive(Clone)]
struct StreamListener {
    callback: f64,
    once: bool,
}

#[derive(Clone)]
struct PipeDestination {
    value: f64,
    end: bool,
}

/// State for a single file stream (read OR write).
pub(crate) struct StreamState {
    kind: StreamKind,
    path: String,
    fd: Option<i32>,
    owner: FdOwner,
    flags: String,
    high_water_mark: usize,
    start: Option<u64>,
    end: Option<u64>,
    position: u64,
    encoding: Option<String>,
    auto_close: bool,
    emit_close: bool,
    listeners: StdHashMap<String, Vec<StreamListener>>,
    pipes: Vec<PipeDestination>,
    object_value: f64,
    opened: bool,
    errored: bool,
    error_msg: Option<String>,
    ended: bool,
    finished: bool,
    closed: bool,
    destroyed: bool,
    paused: bool,
    pumping: bool,
    writable_length: usize,
    writable_need_drain: bool,
    drain_scheduled: bool,
    bytes_read: u64,
    bytes_written: u64,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Utf8ContentMode {
    Utf8,
    Buffer,
}

/// State for `fs.Utf8Stream`. This intentionally stays separate from
/// `StreamState`: Node's fast UTF-8 stream has a much smaller writable/event
/// surface and buffering rules that do not match `fs.WriteStream`.
pub(crate) struct Utf8StreamState {
    fd: i32,
    file: Option<String>,
    pending_file: Option<String>,
    reopen_old_fd: Option<i32>,
    append: bool,
    content_mode: Utf8ContentMode,
    sync: bool,
    fsync: bool,
    min_length: usize,
    max_length: usize,
    max_write: usize,
    periodic_flush: usize,
    periodic_flush_timer: Option<i64>,
    mkdir: bool,
    mode_value: f64,
    retry_eagain: f64,
    custom_fs: f64,
    buffers: Vec<Vec<u8>>,
    len: usize,
    writing: bool,
    opening: bool,
    ending: bool,
    destroyed: bool,
    closed: bool,
    listeners: StdHashMap<String, Vec<StreamListener>>,
    object_value: f64,
}

impl StreamState {
    fn new(kind: StreamKind) -> Self {
        Self {
            kind,
            path: String::new(),
            fd: None,
            owner: FdOwner::Path,
            flags: String::new(),
            high_water_mark: match kind {
                StreamKind::Read => READ_STREAM_DEFAULT_HWM,
                StreamKind::Write => WRITE_STREAM_DEFAULT_HWM,
            },
            start: None,
            end: None,
            position: 0,
            encoding: None,
            auto_close: true,
            emit_close: true,
            listeners: StdHashMap::new(),
            pipes: Vec::new(),
            object_value: f64::from_bits(crate::value::TAG_UNDEFINED),
            opened: false,
            errored: false,
            error_msg: None,
            ended: false,
            finished: false,
            closed: false,
            destroyed: false,
            paused: true,
            pumping: false,
            writable_length: 0,
            writable_need_drain: false,
            drain_scheduled: false,
            bytes_read: 0,
            bytes_written: 0,
        }
    }
}

thread_local! {
    static STREAM_REGISTRY: RefCell<StdHashMap<usize, StreamState>> = RefCell::new(StdHashMap::new());
    static FS_STREAM_NEXT_ID: RefCell<usize> = const { RefCell::new(1) };
    static UTF8_STREAM_REGISTRY: RefCell<StdHashMap<usize, Utf8StreamState>> = RefCell::new(StdHashMap::new());
    static FS_UTF8_STREAM_NEXT_ID: RefCell<usize> = const { RefCell::new(1) };
}

/// Allocate a new stream id and store the initial state.
pub(crate) fn alloc_stream(state: StreamState) -> usize {
    let id = FS_STREAM_NEXT_ID.with(|c| {
        let mut c = c.borrow_mut();
        let id = *c;
        *c += 1;
        id
    });
    STREAM_REGISTRY.with(|r| {
        r.borrow_mut().insert(id, state);
    });
    id
}

fn alloc_utf8_stream(state: Utf8StreamState) -> usize {
    let id = FS_UTF8_STREAM_NEXT_ID.with(|c| {
        let mut c = c.borrow_mut();
        let id = *c;
        *c += 1;
        id
    });
    UTF8_STREAM_REGISTRY.with(|r| {
        r.borrow_mut().insert(id, state);
    });
    id
}

pub(crate) fn scan_fs_stream_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    STREAM_REGISTRY.with(|registry| {
        for state in registry.borrow_mut().values_mut() {
            visitor.visit_nanbox_f64_slot(&mut state.object_value);
            if let FdOwner::FileHandle(handle) = &mut state.owner {
                visitor.visit_nanbox_f64_slot(handle);
            }
            for listeners in state.listeners.values_mut() {
                for listener in listeners {
                    visitor.visit_nanbox_f64_slot(&mut listener.callback);
                }
            }
            for pipe in &mut state.pipes {
                visitor.visit_nanbox_f64_slot(&mut pipe.value);
            }
        }
    });
    UTF8_STREAM_REGISTRY.with(|registry| {
        for state in registry.borrow_mut().values_mut() {
            visitor.visit_nanbox_f64_slot(&mut state.object_value);
            visitor.visit_nanbox_f64_slot(&mut state.retry_eagain);
            visitor.visit_nanbox_f64_slot(&mut state.custom_fs);
            for listeners in state.listeners.values_mut() {
                for listener in listeners {
                    visitor.visit_nanbox_f64_slot(&mut listener.callback);
                }
            }
        }
    });
}

/// Extract a UTF-8 path from a NaN-boxed string value. Returns
/// empty string if the value isn't a string.
pub(crate) fn path_from_value(v: f64) -> String {
    unsafe { decode_path_value(v).unwrap_or_default() }
}

/// Extract raw bytes from strings, Buffer, TypedArray, and DataView-like
/// BufferHeader values.
pub(crate) fn bytes_from_value(v: f64) -> Vec<u8> {
    unsafe {
        if crate::buffer::js_buffer_is_buffer(v.to_bits() as i64) == 1 {
            let buf = buffer_ptr_from_value(v);
            if !buf.is_null() {
                let len = (*buf).length as usize;
                let data = crate::buffer::buffer_data(buf);
                return std::slice::from_raw_parts(data, len).to_vec();
            }
        }
        let bits = v.to_bits();
        let addr = if (bits >> 48) >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else {
            bits as usize
        };
        if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
            let ta = addr as *const crate::typedarray::TypedArrayHeader;
            if let Some(bytes) = crate::typedarray::typed_array_bytes(ta) {
                return bytes.to_vec();
            }
        }
        let ptr = extract_string_ptr(v);
        if ptr.is_null() {
            return Vec::new();
        }
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        std::slice::from_raw_parts(data, len).to_vec()
    }
}

fn is_direct_write_data(value: f64) -> bool {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_any_string() || crate::buffer::js_buffer_is_buffer(value.to_bits() as i64) == 1 {
        return true;
    }
    let bits = value.to_bits();
    let addr = if (bits >> 48) >= 0x7FF8 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        bits as usize
    };
    crate::typedarray::lookup_typed_array_kind(addr).is_some()
}

fn encoding_tag_from_options(options_value: f64) -> i32 {
    let value = JSValue::from_bits(options_value.to_bits());
    if value.is_undefined() || value.is_null() {
        return 0;
    }
    if value.is_any_string() {
        return crate::buffer::js_encoding_tag_from_value(options_value);
    }
    unsafe {
        let Some(enc) = options_field_value(options_value, b"encoding") else {
            return 0;
        };
        let enc_value = f64::from_bits(enc.bits());
        let enc_js = JSValue::from_bits(enc.bits());
        if enc_js.is_undefined() || enc_js.is_null() {
            0
        } else {
            crate::buffer::js_encoding_tag_from_value(enc_value)
        }
    }
}

fn bytes_from_buffer_value(value: f64) -> Vec<u8> {
    unsafe {
        let buf = buffer_ptr_from_value(value);
        if buf.is_null() {
            return Vec::new();
        }
        let len = (*buf).length as usize;
        let data = crate::buffer::buffer_data(buf);
        std::slice::from_raw_parts(data, len).to_vec()
    }
}

fn bytes_from_string_value(value: f64, encoding_tag: i32) -> Vec<u8> {
    let buf = crate::buffer::js_buffer_from_value(value.to_bits() as i64, encoding_tag);
    if buf.is_null() {
        return Vec::new();
    }
    unsafe {
        let len = (*buf).length as usize;
        let data = crate::buffer::buffer_data(buf);
        std::slice::from_raw_parts(data, len).to_vec()
    }
}

mod write_file_input;
pub(crate) use write_file_input::*;

/// Allocate a fresh ClosureHeader whose func_ptr is `func` and
/// whose slot 0 holds the given stream id.
pub(crate) fn make_stream_closure(func: extern "C" fn(), stream_id: usize) -> *mut ClosureHeader {
    let closure = js_closure_alloc(func as *const u8, 1);
    js_closure_set_capture_ptr(closure, 0, stream_id as i64);
    closure
}

#[allow(clippy::type_complexity)]
pub(crate) fn build_stream_object(
    stream_id: usize,
    class_id: u32,
    method_funcs: &[(&str, extern "C" fn())],
) -> *mut ObjectHeader {
    let mut packed: Vec<u8> = Vec::new();
    for (name, _) in method_funcs {
        packed.extend_from_slice(name.as_bytes());
        packed.push(0);
    }
    let field_count = method_funcs.len() as u32;
    let obj = crate::object::js_object_alloc_class_with_keys(
        class_id,
        0,
        field_count,
        packed.as_ptr(),
        (packed.len() - 1) as u32,
    );
    for (i, (_name, func)) in method_funcs.iter().enumerate() {
        let closure = make_stream_closure(*func, stream_id);
        let val = JSValue::pointer(closure as *const u8);
        js_object_set_field(obj, i as u32, val);
    }
    obj
}

#[inline]
pub(crate) fn stream_id_of(closure: *const ClosureHeader) -> usize {
    js_closure_get_capture_ptr(closure, 0) as usize
}

fn undefined_value() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn null_value() -> f64 {
    f64::from_bits(crate::value::TAG_NULL)
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(if value {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    })
}

fn string_value(bytes: &[u8]) -> f64 {
    let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn string_value_str(text: &str) -> f64 {
    string_value(text.as_bytes())
}

fn object_value(obj: *mut ObjectHeader) -> f64 {
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

fn object_ptr_from_value(value: f64) -> Option<*mut ObjectHeader> {
    let js = JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        return None;
    }
    let ptr = js.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        None
    } else {
        Some(ptr)
    }
}

fn current_receiver_value() -> f64 {
    let this_value = crate::object::js_implicit_this_get();
    if object_ptr_from_value(this_value).is_some() {
        this_value
    } else {
        undefined_value()
    }
}

fn set_object_field(obj_value: f64, name: &[u8], value: f64) {
    if let Some(obj) = object_ptr_from_value(obj_value) {
        let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, key, value);
    }
}

fn set_object_field_str(obj_value: f64, name: &[u8], value: &str) {
    set_object_field(obj_value, name, string_value_str(value));
}

fn is_callable_value(value: f64) -> bool {
    !extract_closure_ptr(value).is_null()
}

fn option_bool_default(options_value: f64, field: &[u8], default_value: bool) -> bool {
    unsafe {
        match options_field_value(options_value, field) {
            Some(value) => crate::value::js_is_truthy(f64::from_bits(value.bits())) != 0,
            None => default_value,
        }
    }
}

fn option_usize_default(options_value: f64, field: &[u8], default_value: usize) -> usize {
    unsafe {
        options_number_field(options_value, field)
            .filter(|n| n.is_finite() && *n > 0.0)
            .map(|n| n as usize)
            .unwrap_or(default_value)
    }
}

fn option_u64(options_value: f64, field: &[u8]) -> Option<u64> {
    unsafe {
        options_number_field(options_value, field)
            .filter(|n| n.is_finite() && *n >= 0.0)
            .map(|n| n as u64)
    }
}

fn options_fd(options_value: f64) -> Option<i32> {
    unsafe {
        let value = options_field_value(options_value, b"fd")?;
        numeric_fd_value(f64::from_bits(value.bits()))
    }
}

fn make_flag_value(flag: &str) -> f64 {
    string_value_str(flag)
}

fn current_position_for_fd(fd: i32) -> u64 {
    FD_REGISTRY.with(|registry| {
        registry
            .borrow_mut()
            .get_mut(&fd)
            .and_then(|file| file.stream_position().ok())
            .unwrap_or(0)
    })
}

fn end_position_for_fd(fd: i32) -> u64 {
    FD_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(file) = registry.get_mut(&fd) else {
            return 0;
        };
        let current = file.stream_position().unwrap_or(0);
        let end = file.seek(SeekFrom::End(0)).unwrap_or(current);
        let _ = file.seek(SeekFrom::Start(current));
        end
    })
}

fn fd_append_mode(fd: i32) -> bool {
    FD_APPEND_MODE.with(|flags| flags.borrow().get(&fd).copied().unwrap_or(false))
}

fn update_common_props(state: &StreamState) {
    let obj = state.object_value;
    let fd_value = state.fd.map(|fd| fd as f64).unwrap_or_else(null_value);
    set_object_field(obj, b"fd", fd_value);
    set_object_field_str(obj, b"path", &state.path);
    set_object_field(
        obj,
        b"pending",
        bool_value(!state.opened && state.error_msg.is_none()),
    );
    set_object_field(obj, b"closed", bool_value(state.closed));
    set_object_field(obj, b"destroyed", bool_value(state.destroyed));
    match state.kind {
        StreamKind::Read => {
            set_object_field(
                obj,
                b"readable",
                bool_value(!state.ended && !state.destroyed),
            );
            set_object_field(obj, b"readableEnded", bool_value(state.ended));
            set_object_field(obj, b"readableLength", 0.0);
            set_object_field(obj, b"readableHighWaterMark", state.high_water_mark as f64);
            set_object_field(obj, b"bytesRead", state.bytes_read as f64);
        }
        StreamKind::Write => {
            set_object_field(
                obj,
                b"writable",
                bool_value(!state.finished && !state.destroyed),
            );
            set_object_field(obj, b"writableEnded", bool_value(state.ended));
            set_object_field(obj, b"writableFinished", bool_value(state.finished));
            set_object_field(obj, b"writableLength", state.writable_length as f64);
            set_object_field(
                obj,
                b"writableNeedDrain",
                bool_value(state.writable_need_drain),
            );
            set_object_field(obj, b"writableHighWaterMark", state.high_water_mark as f64);
            set_object_field(obj, b"bytesWritten", state.bytes_written as f64);
        }
    }
}

fn refresh_props(id: usize) {
    STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow().get(&id) {
            update_common_props(state);
        }
    });
}

fn make_error_value(message: &str) -> f64 {
    let msg = message.as_bytes();
    let err_str = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_obj = crate::error::js_error_new_with_message(err_str);
    crate::value::js_nanbox_pointer(err_obj as i64)
}

fn event_name(value: f64) -> String {
    String::from_utf8_lossy(&bytes_from_value(value)).into_owned()
}

fn add_listener(id: usize, event: &str, cb: f64, once: bool) {
    if !is_callable_value(cb) {
        return;
    }
    STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state
                .listeners
                .entry(event.to_string())
                .or_default()
                .push(StreamListener { callback: cb, once });
        }
    });
}

fn callbacks_for_event(id: usize, event: &str) -> Vec<f64> {
    STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(state) = registry.get_mut(&id) else {
            return Vec::new();
        };
        let Some(listeners) = state.listeners.get_mut(event) else {
            return Vec::new();
        };
        let callbacks = listeners.iter().map(|listener| listener.callback).collect();
        listeners.retain(|listener| !listener.once);
        callbacks
    })
}

/// Forward `event` to node:stream's listener registry for this stream's object.
///
/// A read stream carries node:stream's async iterator (installed in
/// `js_fs_create_read_stream`), and that iterator's `data`/`end`/`error`
/// listeners register in node:stream's registry — not the per-id one above. Without
/// this, `for await (const chunk of fs.createReadStream(p))` would hang forever.
fn bridge_to_stream_listeners(id: usize, event: &str, args: &[f64]) {
    let object_value = STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .map(|state| state.object_value)
            .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED))
    });
    if object_value.to_bits() == crate::value::TAG_UNDEFINED {
        return;
    }
    crate::node_stream::emit_to_stream_listeners(object_value, event.as_bytes(), args);
}

fn emit_event0(id: usize, event: &str) {
    use crate::closure::js_closure_call0;
    let callbacks = callbacks_for_event(id, event);
    for cb in callbacks {
        let cb_ptr = extract_closure_ptr(cb);
        if !cb_ptr.is_null() {
            js_closure_call0(cb_ptr);
        }
    }
    bridge_to_stream_listeners(id, event, &[]);
}

fn emit_event1(id: usize, event: &str, arg: f64) {
    use crate::closure::js_closure_call1;
    let callbacks = callbacks_for_event(id, event);
    for cb in callbacks {
        let cb_ptr = extract_closure_ptr(cb);
        if !cb_ptr.is_null() {
            js_closure_call1(cb_ptr, arg);
        }
    }
    bridge_to_stream_listeners(id, event, &[arg]);
}

fn call_js_method0(receiver: f64, name: &[u8]) -> f64 {
    unsafe {
        crate::object::js_native_call_method(
            receiver,
            name.as_ptr() as *const i8,
            name.len(),
            std::ptr::null(),
            0,
        )
    }
}

fn call_js_method1(receiver: f64, name: &[u8], arg0: f64) -> f64 {
    let args = [arg0];
    unsafe {
        crate::object::js_native_call_method(
            receiver,
            name.as_ptr() as *const i8,
            name.len(),
            args.as_ptr(),
            args.len(),
        )
    }
}

fn call_js_method2(receiver: f64, name: &[u8], arg0: f64, arg1: f64) -> f64 {
    let args = [arg0, arg1];
    unsafe {
        crate::object::js_native_call_method(
            receiver,
            name.as_ptr() as *const i8,
            name.len(),
            args.as_ptr(),
            args.len(),
        )
    }
}

fn emit_stored_error(id: usize) {
    let error_value = STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        registry
            .get(&id)
            .and_then(|state| state.error_msg.as_deref())
            .map(make_error_value)
    });
    if let Some(err) = error_value {
        emit_event1(id, "error", err);
    }
}

fn record_stream_error(id: usize, message: String) {
    STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.errored = true;
            state.error_msg = Some(message);
        }
    });
    refresh_props(id);
    emit_stored_error(id);
}

fn close_fd_for_state(state: &mut StreamState) {
    let Some(fd) = state.fd else {
        state.closed = true;
        return;
    };
    if fd_is_registered(fd) {
        match state.owner {
            FdOwner::FileHandle(handle) => close_filehandle_fd(fd, handle),
            FdOwner::Path | FdOwner::External => {
                let _ = js_fs_close_sync(fd as f64);
            }
        }
    }
    state.fd = None;
    state.closed = true;
}

fn maybe_close_stream(id: usize, force: bool) {
    // `Some(emit_close)` when the stream transitioned to closed in THIS call.
    let closed_now = STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(state) = registry.get_mut(&id) else {
            return None;
        };
        if state.closed {
            return None;
        }
        if !force && !state.auto_close {
            return None;
        }
        close_fd_for_state(state);
        update_common_props(state);
        Some(state.emit_close)
    });
    let Some(should_emit_close) = closed_now else {
        return;
    };
    if should_emit_close {
        emit_event0(id, "close");
    }
    // 2026-07-09 GC audit wave 2: the state is terminal — the fd is closed
    // and 'close' has been delivered — but the registry record previously
    // kept EVERY GC-rooted value alive forever (listener closures, pipe
    // targets, and the stream object itself via `object_value`, all visited
    // by `scan_fs_stream_roots_mut`). Release them now so the stream's
    // object graph becomes collectable. The slim record itself stays so the
    // late-listener replay arms in `stream_on_common`
    // ('error'/'end'/'finish'/'close') keep answering from the terminal
    // booleans + `error_msg`; those read no rooted values.
    STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.listeners.clear();
            state.pipes.clear();
            state.object_value = f64::from_bits(crate::value::TAG_UNDEFINED);
            if let FdOwner::FileHandle(handle) = &mut state.owner {
                *handle = f64::from_bits(crate::value::TAG_UNDEFINED);
            }
        }
    });
}

fn normalize_write_args(chunk: f64, encoding: f64, cb: f64) -> (Option<f64>, Option<f64>) {
    if is_callable_value(chunk) {
        return (None, Some(chunk));
    }
    if is_callable_value(encoding) {
        return (Some(chunk), Some(encoding));
    }
    let callback = if is_callable_value(cb) {
        Some(cb)
    } else {
        None
    };
    let value = JSValue::from_bits(chunk.to_bits());
    if value.is_null() || value.is_undefined() {
        (None, callback)
    } else {
        (Some(chunk), callback)
    }
}

fn write_to_stream_fd(id: usize, bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty() {
        return Ok(());
    }
    let (fd, position, append) = STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let Some(state) = registry.get(&id) else {
            return (None, 0, false);
        };
        (
            state.fd,
            state.position,
            matches!(state.flags.as_str(), "a" | "a+" | "ax" | "ax+")
                || state.fd.is_some_and(fd_append_mode),
        )
    });
    let Some(fd) = fd else {
        return Err("bad file descriptor".to_string());
    };
    let result = FD_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(file) = registry.get_mut(&fd) else {
            return Err("bad file descriptor".to_string());
        };
        if append {
            file.seek(SeekFrom::End(0)).map_err(|err| err.to_string())?;
        } else {
            file.seek(SeekFrom::Start(position))
                .map_err(|err| err.to_string())?;
        }
        file.write_all(bytes).map_err(|err| err.to_string())
    });
    if result.is_ok() {
        STREAM_REGISTRY.with(|registry| {
            if let Some(state) = registry.borrow_mut().get_mut(&id) {
                state.position = state.position.saturating_add(bytes.len() as u64);
                state.bytes_written = state.bytes_written.saturating_add(bytes.len() as u64);
            }
        });
        refresh_props(id);
    }
    result
}

fn schedule_drain(id: usize) {
    let should_schedule = STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(state) = registry.get_mut(&id) else {
            return false;
        };
        if state.drain_scheduled || !state.writable_need_drain {
            return false;
        }
        state.drain_scheduled = true;
        true
    });
    if should_schedule {
        let closure = js_closure_alloc(write_stream_drain_timer_impl as *const u8, 1);
        js_closure_set_capture_ptr(closure, 0, id as i64);
        let _ = crate::timer::js_set_timeout_callback(closure as i64, 0.0);
    }
}

fn flush_write_drain(id: usize) {
    let should_emit = STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(state) = registry.get_mut(&id) else {
            return false;
        };
        if !state.writable_need_drain {
            state.drain_scheduled = false;
            return false;
        }
        state.writable_length = 0;
        state.writable_need_drain = false;
        state.drain_scheduled = false;
        update_common_props(state);
        true
    });
    if should_emit {
        emit_event0(id, "drain");
    }
}

extern "C" fn write_stream_drain_timer_impl(closure: *const ClosureHeader) -> f64 {
    flush_write_drain(stream_id_of(closure));
    undefined_value()
}

pub(crate) extern "C" fn write_stream_write_impl(
    closure: *const ClosureHeader,
    chunk: f64,
    encoding: f64,
    cb: f64,
) -> f64 {
    use crate::closure::js_closure_call0;
    let id = stream_id_of(closure);
    let (chunk_value, callback) = normalize_write_args(chunk, encoding, cb);
    let Some(chunk_value) = chunk_value else {
        if let Some(callback) = callback {
            let cb_ptr = extract_closure_ptr(callback);
            if !cb_ptr.is_null() {
                js_closure_call0(cb_ptr);
            }
        }
        return bool_value(true);
    };
    let bytes = bytes_from_value(chunk_value);
    let (should_return, should_write) = STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(state) = registry.get_mut(&id) else {
            return (true, false);
        };
        if state.kind != StreamKind::Write || state.finished || state.destroyed {
            return (false, false);
        }
        state.writable_length = state.writable_length.saturating_add(bytes.len());
        let over_hwm = state.writable_length >= state.high_water_mark;
        if over_hwm {
            state.writable_need_drain = true;
        }
        update_common_props(state);
        (!over_hwm, true)
    });
    if should_write {
        if let Err(message) = write_to_stream_fd(id, &bytes) {
            record_stream_error(id, message);
        }
    }
    if should_return {
        STREAM_REGISTRY.with(|registry| {
            if let Some(state) = registry.borrow_mut().get_mut(&id) {
                state.writable_length = 0;
                update_common_props(state);
            }
        });
    } else {
        schedule_drain(id);
    }
    if let Some(callback) = callback {
        let cb_ptr = extract_closure_ptr(callback);
        if !cb_ptr.is_null() {
            js_closure_call0(cb_ptr);
        }
    }
    bool_value(should_return)
}

pub(crate) extern "C" fn write_stream_end_impl(
    closure: *const ClosureHeader,
    chunk: f64,
    encoding: f64,
    cb: f64,
) -> f64 {
    use crate::closure::js_closure_call0;
    let id = stream_id_of(closure);
    let (chunk_value, callback) = normalize_write_args(chunk, encoding, cb);
    if let Some(chunk_value) = chunk_value {
        let bytes = bytes_from_value(chunk_value);
        if let Err(message) = write_to_stream_fd(id, &bytes) {
            record_stream_error(id, message);
        }
    }
    flush_write_drain(id);
    let should_finish = STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(state) = registry.get_mut(&id) else {
            return false;
        };
        if state.finished {
            return false;
        }
        state.ended = true;
        state.finished = state.error_msg.is_none();
        state.writable_length = 0;
        state.writable_need_drain = false;
        update_common_props(state);
        state.error_msg.is_none()
    });
    if should_finish {
        if let Some(callback) = callback {
            let cb_ptr = extract_closure_ptr(callback);
            if !cb_ptr.is_null() {
                js_closure_call0(cb_ptr);
            }
        }
        emit_event0(id, "finish");
    } else {
        emit_stored_error(id);
    }
    maybe_close_stream(id, false);
    current_receiver_value()
}

pub(crate) extern "C" fn write_stream_on_impl(
    closure: *const ClosureHeader,
    event: f64,
    cb: f64,
) -> f64 {
    stream_on_common(stream_id_of(closure), event, cb, false);
    current_receiver_value()
}

pub(crate) extern "C" fn write_stream_once_impl(
    closure: *const ClosureHeader,
    event: f64,
    cb: f64,
) -> f64 {
    stream_on_common(stream_id_of(closure), event, cb, true);
    current_receiver_value()
}

pub(crate) extern "C" fn stream_emit_impl(
    closure: *const ClosureHeader,
    event: f64,
    arg: f64,
) -> f64 {
    let id = stream_id_of(closure);
    let name = event_name(event);
    if arg.to_bits() == crate::value::TAG_UNDEFINED {
        emit_event0(id, &name);
    } else {
        emit_event1(id, &name, arg);
    }
    bool_value(true)
}

fn throw_plain_type_error_value(message: &str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

mod utf8_stream;
pub(crate) use utf8_stream::*;

pub(crate) extern "C" fn write_stream_close_impl(closure: *const ClosureHeader, cb: f64) -> f64 {
    let id = stream_id_of(closure);
    if is_callable_value(cb) {
        add_listener(id, "close", cb, true);
    }
    STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.destroyed = true;
            update_common_props(state);
        }
    });
    maybe_close_stream(id, true);
    current_receiver_value()
}

fn read_chunk_value(bytes: &[u8], encoding: Option<&str>) -> f64 {
    if let Some(encoding) = encoding {
        let ptr = encoded_string_ptr(bytes, encoding);
        f64::from_bits(JSValue::string_ptr(ptr).bits())
    } else {
        buffer_value_from_bytes(bytes)
    }
}

fn read_next_chunk(id: usize) -> Result<Option<(Vec<u8>, Option<String>)>, String> {
    let (fd, pos, amount, encoding) = STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let Some(state) = registry.get(&id) else {
            return (None, 0, 0, None);
        };
        if state.kind != StreamKind::Read || state.ended || state.destroyed {
            return (None, 0, 0, None);
        }
        if let Some(end) = state.end {
            if state.position > end {
                return (state.fd, state.position, 0, state.encoding.clone());
            }
        }
        let mut amount = state.high_water_mark.max(1);
        if let Some(end) = state.end {
            let remaining = end.saturating_sub(state.position).saturating_add(1);
            amount = amount.min(remaining as usize);
        }
        (state.fd, state.position, amount, state.encoding.clone())
    });
    if amount == 0 {
        return Ok(None);
    }
    let Some(fd) = fd else {
        return Err("bad file descriptor".to_string());
    };
    let result = FD_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(file) = registry.get_mut(&fd) else {
            return Err("bad file descriptor".to_string());
        };
        file.seek(SeekFrom::Start(pos))
            .map_err(|err| err.to_string())?;
        let mut buffer = vec![0; amount];
        let read = file.read(&mut buffer).map_err(|err| err.to_string())?;
        buffer.truncate(read);
        Ok(buffer)
    })?;
    if result.is_empty() {
        return Ok(None);
    }
    STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.position = state.position.saturating_add(result.len() as u64);
            state.bytes_read = state.bytes_read.saturating_add(result.len() as u64);
            update_common_props(state);
        }
    });
    Ok(Some((result, encoding)))
}

fn finish_read_stream(id: usize) {
    let should_emit = STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(state) = registry.get_mut(&id) else {
            return false;
        };
        if state.ended {
            return false;
        }
        state.ended = true;
        state.paused = true;
        update_common_props(state);
        true
    });
    if should_emit {
        emit_event0(id, "end");
        maybe_close_stream(id, false);
    }
}

fn install_pipe_drain_resume(source_id: usize, dest: f64) {
    let closure = js_closure_alloc(read_stream_resume_from_drain_impl as *const u8, 1);
    js_closure_set_capture_ptr(closure, 0, source_id as i64);
    let listener = f64::from_bits(JSValue::pointer(closure as *const u8).bits());
    let _ = call_js_method2(dest, b"once", string_value(b"drain"), listener);
}

extern "C" fn read_stream_resume_from_drain_impl(closure: *const ClosureHeader) -> f64 {
    let id = stream_id_of(closure);
    STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.paused = false;
        }
    });
    read_stream_pump(id);
    undefined_value()
}

fn write_to_pipes(id: usize, chunk: f64) {
    let pipes = STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .map(|state| state.pipes.clone())
            .unwrap_or_default()
    });
    for pipe in pipes {
        let ret = call_js_method1(pipe.value, b"write", chunk);
        if ret.to_bits() == crate::value::TAG_FALSE {
            STREAM_REGISTRY.with(|registry| {
                if let Some(state) = registry.borrow_mut().get_mut(&id) {
                    state.paused = true;
                }
            });
            install_pipe_drain_resume(id, pipe.value);
            break;
        }
    }
}

fn end_pipes(id: usize) {
    let pipes = STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .map(|state| state.pipes.clone())
            .unwrap_or_default()
    });
    for pipe in pipes {
        if pipe.end {
            let _ = call_js_method0(pipe.value, b"end");
        }
    }
}

fn read_stream_pump(id: usize) {
    let should_start = STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(state) = registry.get_mut(&id) else {
            return false;
        };
        if state.kind != StreamKind::Read
            || state.paused
            || state.pumping
            || state.ended
            || state.destroyed
            || state.error_msg.is_some()
        {
            return false;
        }
        state.pumping = true;
        true
    });
    if !should_start {
        return;
    }
    loop {
        let result = read_next_chunk(id);
        match result {
            Ok(Some((bytes, encoding))) => {
                let chunk = read_chunk_value(&bytes, encoding.as_deref());
                emit_event1(id, "data", chunk);
                write_to_pipes(id, chunk);
                let keep_going = STREAM_REGISTRY.with(|registry| {
                    let registry = registry.borrow();
                    let Some(state) = registry.get(&id) else {
                        return false;
                    };
                    !state.paused && !state.ended && !state.destroyed && state.error_msg.is_none()
                });
                if !keep_going {
                    break;
                }
            }
            Ok(None) => {
                STREAM_REGISTRY.with(|registry| {
                    if let Some(state) = registry.borrow_mut().get_mut(&id) {
                        state.pumping = false;
                    }
                });
                end_pipes(id);
                finish_read_stream(id);
                return;
            }
            Err(message) => {
                STREAM_REGISTRY.with(|registry| {
                    if let Some(state) = registry.borrow_mut().get_mut(&id) {
                        state.pumping = false;
                    }
                });
                record_stream_error(id, message);
                maybe_close_stream(id, false);
                return;
            }
        }
    }
    STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.pumping = false;
        }
    });
}

pub(crate) extern "C" fn read_stream_on_impl(
    closure: *const ClosureHeader,
    event: f64,
    cb: f64,
) -> f64 {
    let id = stream_id_of(closure);
    stream_on_common(id, event, cb, false);
    if event_name(event) == "data" {
        STREAM_REGISTRY.with(|registry| {
            if let Some(state) = registry.borrow_mut().get_mut(&id) {
                state.paused = false;
            }
        });
        read_stream_pump(id);
    }
    current_receiver_value()
}

pub(crate) extern "C" fn read_stream_once_impl(
    closure: *const ClosureHeader,
    event: f64,
    cb: f64,
) -> f64 {
    let id = stream_id_of(closure);
    stream_on_common(id, event, cb, true);
    if event_name(event) == "data" {
        STREAM_REGISTRY.with(|registry| {
            if let Some(state) = registry.borrow_mut().get_mut(&id) {
                state.paused = false;
            }
        });
        read_stream_pump(id);
    }
    current_receiver_value()
}

pub(crate) extern "C" fn read_stream_pipe_impl(
    closure: *const ClosureHeader,
    dest: f64,
    options: f64,
) -> f64 {
    let id = stream_id_of(closure);
    let pipe_end = option_bool_default(options, b"end", true);
    STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.pipes.push(PipeDestination {
                value: dest,
                end: pipe_end,
            });
            state.paused = false;
        }
    });
    read_stream_pump(id);
    dest
}

pub(crate) extern "C" fn read_stream_pause_impl(closure: *const ClosureHeader) -> f64 {
    STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&stream_id_of(closure)) {
            state.paused = true;
        }
    });
    current_receiver_value()
}

pub(crate) extern "C" fn read_stream_resume_impl(closure: *const ClosureHeader) -> f64 {
    let id = stream_id_of(closure);
    STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.paused = false;
        }
    });
    read_stream_pump(id);
    current_receiver_value()
}

pub(crate) extern "C" fn read_stream_is_paused_impl(closure: *const ClosureHeader) -> f64 {
    let paused = STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&stream_id_of(closure))
            .map(|state| state.paused)
            .unwrap_or(true)
    });
    bool_value(paused)
}

pub(crate) extern "C" fn read_stream_close_impl(closure: *const ClosureHeader, cb: f64) -> f64 {
    let id = stream_id_of(closure);
    if is_callable_value(cb) {
        add_listener(id, "close", cb, true);
    }
    STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.destroyed = true;
            state.paused = true;
            update_common_props(state);
        }
    });
    maybe_close_stream(id, true);
    current_receiver_value()
}

fn stream_on_common(id: usize, event_value: f64, cb: f64, once: bool) {
    let event = event_name(event_value);
    let immediate = STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let Some(state) = registry.get(&id) else {
            return None;
        };
        match event.as_str() {
            "open"
                if state.opened
                    && !matches!(state.owner, FdOwner::External | FdOwner::FileHandle(_)) =>
            {
                state.fd.map(|fd| ("open", fd as f64))
            }
            "ready" if state.opened => Some(("ready", undefined_value())),
            "error" => state
                .error_msg
                .as_deref()
                .map(|message| ("error", make_error_value(message))),
            "end" if state.kind == StreamKind::Read && state.ended => {
                Some(("end", undefined_value()))
            }
            "finish" if state.kind == StreamKind::Write && state.finished => {
                Some(("finish", undefined_value()))
            }
            "close" if state.closed && state.emit_close => Some(("close", undefined_value())),
            _ => None,
        }
    });
    if let Some((name, arg)) = immediate {
        if is_callable_value(cb) {
            let cb_ptr = extract_closure_ptr(cb);
            if !cb_ptr.is_null() {
                if name == "open" || name == "error" {
                    crate::closure::js_closure_call1(cb_ptr, arg);
                } else {
                    crate::closure::js_closure_call0(cb_ptr);
                }
            }
        }
        return;
    }
    add_listener(id, &event, cb, once);
}

/// Extract a raw ClosureHeader pointer from a NaN-boxed f64.
pub(crate) fn extract_closure_ptr(v: f64) -> *const ClosureHeader {
    let bits = v.to_bits();
    let top16 = bits >> 48;
    let raw = if (0x7FF8..=0x7FFF).contains(&top16) {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if top16 == 0 {
        bits as usize
    } else {
        return std::ptr::null();
    };
    if raw < 0x1000 || !crate::closure::is_closure_ptr(raw) {
        std::ptr::null()
    } else {
        raw as *const ClosureHeader
    }
}

fn register_stream_method_arities() {
    crate::closure::js_register_closure_arity(write_stream_write_impl as *const u8, 3);
    crate::closure::js_register_closure_arity(write_stream_end_impl as *const u8, 3);
    crate::closure::js_register_closure_arity(write_stream_on_impl as *const u8, 2);
    crate::closure::js_register_closure_arity(write_stream_once_impl as *const u8, 2);
    crate::closure::js_register_closure_arity(write_stream_close_impl as *const u8, 1);
    crate::closure::js_register_closure_arity(stream_emit_impl as *const u8, 2);
    crate::closure::js_register_closure_arity(write_stream_drain_timer_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(read_stream_on_impl as *const u8, 2);
    crate::closure::js_register_closure_arity(read_stream_once_impl as *const u8, 2);
    crate::closure::js_register_closure_arity(read_stream_pipe_impl as *const u8, 2);
    crate::closure::js_register_closure_arity(read_stream_pause_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(read_stream_resume_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(read_stream_is_paused_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(read_stream_close_impl as *const u8, 1);
    crate::closure::js_register_closure_arity(read_stream_resume_from_drain_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(utf8_stream_write_impl as *const u8, 1);
    crate::closure::js_register_closure_arity(utf8_stream_flush_impl as *const u8, 1);
    crate::closure::js_register_closure_arity(utf8_stream_flush_sync_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(utf8_stream_end_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(utf8_stream_destroy_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(utf8_stream_reopen_impl as *const u8, 1);
    crate::closure::js_register_closure_arity(utf8_stream_on_impl as *const u8, 2);
    crate::closure::js_register_closure_arity(utf8_stream_once_impl as *const u8, 2);
    crate::closure::js_register_closure_arity(utf8_stream_off_impl as *const u8, 2);
    crate::closure::js_register_closure_arity(utf8_stream_remove_all_impl as *const u8, 1);
    crate::closure::js_register_closure_arity(utf8_stream_listener_count_impl as *const u8, 1);
    crate::closure::js_register_closure_arity(utf8_stream_emit_impl as *const u8, 2);
    crate::closure::js_register_closure_arity(utf8_periodic_flush_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(utf8_async_open_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(utf8_async_open_done_impl as *const u8, 2);
    crate::closure::js_register_closure_arity(utf8_async_mkdir_done_impl as *const u8, 1);
    crate::closure::js_register_closure_arity(utf8_close_events_impl as *const u8, 0);
}

fn init_read_state_from_options(
    path_value: f64,
    options_value: f64,
    supplied_fd: Option<(i32, Option<f64>)>,
) -> StreamState {
    let mut state = StreamState::new(StreamKind::Read);
    state.path = path_from_value(path_value);
    state.flags = file_options_flag(options_value, "r");
    state.high_water_mark =
        option_usize_default(options_value, b"highWaterMark", READ_STREAM_DEFAULT_HWM);
    state.start = option_u64(options_value, b"start");
    state.end = option_u64(options_value, b"end");
    state.position = state.start.unwrap_or(0);
    state.encoding = fs_encoding_option(options_value).filter(|encoding| encoding != "buffer");
    state.auto_close = option_bool_default(options_value, b"autoClose", true);
    state.emit_close = option_bool_default(options_value, b"emitClose", true);

    if let Some((fd, handle)) =
        supplied_fd.or_else(|| options_fd(options_value).map(|fd| (fd, None)))
    {
        state.fd = Some(fd);
        state.owner = handle.map(FdOwner::FileHandle).unwrap_or(FdOwner::External);
        state.position = state.start.unwrap_or_else(|| current_position_for_fd(fd));
        state.opened = fd_is_registered(fd);
        if !state.opened {
            state.error_msg = Some("bad file descriptor".to_string());
        }
        return state;
    }

    if let Some(fd) = numeric_fd_value(path_value) {
        state.fd = Some(fd);
        state.owner = FdOwner::External;
        state.position = state.start.unwrap_or_else(|| current_position_for_fd(fd));
        state.opened = fd_is_registered(fd);
        if !state.opened {
            state.error_msg = Some("bad file descriptor".to_string());
        }
        return state;
    }

    let flag_value = make_flag_value(&state.flags);
    match unsafe { fs_open_sync_result(path_value, flag_value) } {
        Ok(fd) => {
            state.fd = Some(fd);
            state.owner = FdOwner::Path;
            state.opened = true;
        }
        Err((err, _path)) => {
            state.error_msg = Some(err.to_string());
        }
    }
    state
}

fn init_write_state_from_options(
    path_value: f64,
    options_value: f64,
    supplied_fd: Option<(i32, Option<f64>)>,
) -> StreamState {
    let mut state = StreamState::new(StreamKind::Write);
    state.path = path_from_value(path_value);
    state.flags = file_options_flag(options_value, "w");
    state.high_water_mark =
        option_usize_default(options_value, b"highWaterMark", WRITE_STREAM_DEFAULT_HWM);
    state.start = option_u64(options_value, b"start");
    state.position = state.start.unwrap_or(0);
    state.auto_close = option_bool_default(options_value, b"autoClose", true);
    state.emit_close = option_bool_default(options_value, b"emitClose", true);

    if let Some((fd, handle)) =
        supplied_fd.or_else(|| options_fd(options_value).map(|fd| (fd, None)))
    {
        state.fd = Some(fd);
        state.owner = handle.map(FdOwner::FileHandle).unwrap_or(FdOwner::External);
        state.opened = fd_is_registered(fd);
        state.position =
            if matches!(state.flags.as_str(), "a" | "a+" | "ax" | "ax+") || fd_append_mode(fd) {
                end_position_for_fd(fd)
            } else {
                state.start.unwrap_or_else(|| current_position_for_fd(fd))
            };
        if !state.opened {
            state.error_msg = Some("bad file descriptor".to_string());
        }
        return state;
    }

    if let Some(fd) = numeric_fd_value(path_value) {
        state.fd = Some(fd);
        state.owner = FdOwner::External;
        state.opened = fd_is_registered(fd);
        state.position = state.start.unwrap_or_else(|| current_position_for_fd(fd));
        if !state.opened {
            state.error_msg = Some("bad file descriptor".to_string());
        }
        return state;
    }

    let flag_value = make_flag_value(&state.flags);
    match unsafe { fs_open_sync_result(path_value, flag_value) } {
        Ok(fd) => {
            state.fd = Some(fd);
            state.owner = FdOwner::Path;
            state.opened = true;
            if matches!(state.flags.as_str(), "a" | "a+" | "ax" | "ax+") {
                state.position = end_position_for_fd(fd);
            }
        }
        Err((err, _path)) => {
            state.error_msg = Some(err.to_string());
        }
    }
    state
}

fn create_write_stream_with_state(state: StreamState) -> f64 {
    register_stream_method_arities();
    let id = alloc_stream(state);
    let method_funcs: [(&str, extern "C" fn()); 8] = [
        ("write", unsafe {
            std::mem::transmute::<
                extern "C" fn(*const ClosureHeader, f64, f64, f64) -> f64,
                extern "C" fn(),
            >(write_stream_write_impl)
        }),
        ("end", unsafe {
            std::mem::transmute::<
                extern "C" fn(*const ClosureHeader, f64, f64, f64) -> f64,
                extern "C" fn(),
            >(write_stream_end_impl)
        }),
        ("on", unsafe {
            std::mem::transmute::<
                extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
                extern "C" fn(),
            >(write_stream_on_impl)
        }),
        ("once", unsafe {
            std::mem::transmute::<
                extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
                extern "C" fn(),
            >(write_stream_once_impl)
        }),
        ("addListener", unsafe {
            std::mem::transmute::<
                extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
                extern "C" fn(),
            >(write_stream_on_impl)
        }),
        ("close", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader, f64) -> f64, extern "C" fn()>(
                write_stream_close_impl,
            )
        }),
        ("destroy", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader, f64) -> f64, extern "C" fn()>(
                write_stream_close_impl,
            )
        }),
        ("emit", unsafe {
            std::mem::transmute::<
                extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
                extern "C" fn(),
            >(stream_emit_impl)
        }),
    ];
    let obj = build_stream_object(id, CLASS_ID_FS_WRITE_STREAM, &method_funcs);
    let value = object_value(obj);
    STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.object_value = value;
            update_common_props(state);
        }
    });
    value
}

fn create_read_stream_with_state(state: StreamState) -> f64 {
    register_stream_method_arities();
    let id = alloc_stream(state);
    let method_funcs: [(&str, extern "C" fn()); 10] = [
        ("on", unsafe {
            std::mem::transmute::<
                extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
                extern "C" fn(),
            >(read_stream_on_impl)
        }),
        ("once", unsafe {
            std::mem::transmute::<
                extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
                extern "C" fn(),
            >(read_stream_once_impl)
        }),
        ("addListener", unsafe {
            std::mem::transmute::<
                extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
                extern "C" fn(),
            >(read_stream_on_impl)
        }),
        ("pipe", unsafe {
            std::mem::transmute::<
                extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
                extern "C" fn(),
            >(read_stream_pipe_impl)
        }),
        ("pause", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader) -> f64, extern "C" fn()>(
                read_stream_pause_impl,
            )
        }),
        ("resume", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader) -> f64, extern "C" fn()>(
                read_stream_resume_impl,
            )
        }),
        ("isPaused", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader) -> f64, extern "C" fn()>(
                read_stream_is_paused_impl,
            )
        }),
        ("close", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader, f64) -> f64, extern "C" fn()>(
                read_stream_close_impl,
            )
        }),
        ("destroy", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader, f64) -> f64, extern "C" fn()>(
                read_stream_close_impl,
            )
        }),
        ("emit", unsafe {
            std::mem::transmute::<
                extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
                extern "C" fn(),
            >(stream_emit_impl)
        }),
    ];
    let obj = build_stream_object(id, CLASS_ID_FS_READ_STREAM, &method_funcs);
    let value = object_value(obj);
    STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.object_value = value;
            update_common_props(state);
        }
    });
    // Like Node's, a read stream must be async-iterable: `for await (const chunk of
    // fs.createReadStream(p))`, and the `typeof stream[Symbol.asyncIterator] ===
    // "function"` probe that stream-consuming libraries run before accepting a
    // stream at all. `emit_event0`/`emit_event1` forward to node:stream's listener
    // registry so the iterator this installs actually receives the chunks.
    crate::node_stream::async_iterator::install_foreign_readable_async_iterator_symbol(value);
    value
}

fn install_utf8_stream_dispose_symbol(value: f64, method: f64) {
    let dispose = crate::symbol::well_known_symbol("dispose");
    if dispose.is_null() {
        return;
    }
    let symbol_value = f64::from_bits(JSValue::pointer(dispose as *const u8).bits());
    unsafe {
        crate::symbol::js_object_set_symbol_property(value, symbol_value, method);
    }
}

fn create_utf8_stream_with_state(state: Utf8StreamState) -> f64 {
    register_stream_method_arities();
    let periodic_flush = state.periodic_flush;
    let schedule_open = state.opening;
    let id = alloc_utf8_stream(state);
    let method_funcs: [(&str, extern "C" fn()); 16] = [
        ("write", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader, f64) -> f64, extern "C" fn()>(
                utf8_stream_write_impl,
            )
        }),
        ("flush", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader, f64) -> f64, extern "C" fn()>(
                utf8_stream_flush_impl,
            )
        }),
        ("flushSync", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader) -> f64, extern "C" fn()>(
                utf8_stream_flush_sync_impl,
            )
        }),
        ("end", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader) -> f64, extern "C" fn()>(
                utf8_stream_end_impl,
            )
        }),
        ("destroy", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader) -> f64, extern "C" fn()>(
                utf8_stream_destroy_impl,
            )
        }),
        ("reopen", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader, f64) -> f64, extern "C" fn()>(
                utf8_stream_reopen_impl,
            )
        }),
        ("on", unsafe {
            std::mem::transmute::<
                extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
                extern "C" fn(),
            >(utf8_stream_on_impl)
        }),
        ("once", unsafe {
            std::mem::transmute::<
                extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
                extern "C" fn(),
            >(utf8_stream_once_impl)
        }),
        ("addListener", unsafe {
            std::mem::transmute::<
                extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
                extern "C" fn(),
            >(utf8_stream_on_impl)
        }),
        ("off", unsafe {
            std::mem::transmute::<
                extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
                extern "C" fn(),
            >(utf8_stream_off_impl)
        }),
        ("removeListener", unsafe {
            std::mem::transmute::<
                extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
                extern "C" fn(),
            >(utf8_stream_off_impl)
        }),
        ("removeAllListeners", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader, f64) -> f64, extern "C" fn()>(
                utf8_stream_remove_all_impl,
            )
        }),
        ("listenerCount", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader, f64) -> f64, extern "C" fn()>(
                utf8_stream_listener_count_impl,
            )
        }),
        ("emit", unsafe {
            std::mem::transmute::<
                extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
                extern "C" fn(),
            >(utf8_stream_emit_impl)
        }),
        ("close", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader) -> f64, extern "C" fn()>(
                utf8_stream_destroy_impl,
            )
        }),
        ("@@__perry_wk_dispose", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader) -> f64, extern "C" fn()>(
                utf8_stream_destroy_impl,
            )
        }),
    ];
    let obj = build_stream_object(id, CLASS_ID_FS_UTF8_STREAM, &method_funcs);
    let value = object_value(obj);
    let dispose_name = b"@@__perry_wk_dispose";
    let dispose_key = js_string_from_bytes(dispose_name.as_ptr(), dispose_name.len() as u32);
    let dispose_method = crate::object::js_object_get_field_by_name(obj, dispose_key);
    install_utf8_stream_dispose_symbol(value, f64::from_bits(dispose_method.bits()));
    UTF8_STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.object_value = value;
            update_utf8_props(state);
        }
    });
    if schedule_open {
        utf8_start_async_open(id);
    }
    if periodic_flush > 0 {
        let closure = js_closure_alloc(utf8_periodic_flush_impl as *const u8, 1);
        js_closure_set_capture_ptr(closure, 0, id as i64);
        let timer = crate::timer::setInterval(closure as i64, periodic_flush as f64);
        crate::timer::js_timer_unref(timer);
        UTF8_STREAM_REGISTRY.with(|registry| {
            if let Some(state) = registry.borrow_mut().get_mut(&id) {
                state.periodic_flush_timer = Some(timer);
            }
        });
    }
    if !schedule_open {
        utf8_emit_event0(id, "ready");
    }
    value
}

#[no_mangle]
pub extern "C" fn js_fs_create_write_stream(path_value: f64, options_value: f64) -> f64 {
    let state = init_write_state_from_options(path_value, options_value, None);
    create_write_stream_with_state(state)
}

#[no_mangle]
pub extern "C" fn js_fs_create_read_stream(path_value: f64, options_value: f64) -> f64 {
    let state = init_read_state_from_options(path_value, options_value, None);
    create_read_stream_with_state(state)
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_new(options_value: f64) -> f64 {
    let state = utf8_initial_state(options_value);
    create_utf8_stream_with_state(state)
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_call_without_new(_options_value: f64) -> f64 {
    throw_plain_type_error_value("Class constructor Utf8Stream cannot be invoked without 'new'")
}

pub(crate) fn js_fs_create_read_stream_from_filehandle(
    path_value: f64,
    fd: i32,
    handle: f64,
    options_value: f64,
) -> f64 {
    let state = init_read_state_from_options(path_value, options_value, Some((fd, Some(handle))));
    create_read_stream_with_state(state)
}

pub(crate) fn js_fs_create_write_stream_from_filehandle(
    path_value: f64,
    fd: i32,
    handle: f64,
    options_value: f64,
) -> f64 {
    let state = init_write_state_from_options(path_value, options_value, Some((fd, Some(handle))));
    create_write_stream_with_state(state)
}
