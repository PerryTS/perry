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

/// #9493: a chunk `WriteStream.write()`/`end()` accepted, awaiting its turn.
struct PendingWrite {
    bytes: Vec<u8>,
    /// `write(chunk, cb)`'s callback; `undefined` when absent.
    callback: f64,
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
    /// #9493: chunks accepted by `write()`/`end()` and not yet written. Node
    /// hands each to the thread pool; perry performs them on a later
    /// event-loop turn (`run_write_stream_turn`), so a `process.exit()` in the
    /// same tick abandons them exactly as Node does.
    pending_writes: Vec<PendingWrite>,
    /// #9493: `end(cb)`'s callback — runs before `'finish'` (Node's
    /// `kOnFinished`). `undefined` when absent.
    end_callback: f64,
    /// #9493: node-shaped error value (`.code`/`.syscall`/`.path`) from the
    /// deferred open, handed to the pending callbacks and to `'error'`.
    /// `undefined` until then; `error_msg` stays the "errored" flag.
    error_value: f64,
    /// #9493: a turn is already parked on the callback-timer queue.
    turn_pending: bool,
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
            pending_writes: Vec::new(),
            end_callback: f64::from_bits(crate::value::TAG_UNDEFINED),
            error_value: f64::from_bits(crate::value::TAG_UNDEFINED),
            turn_pending: false,
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
            // #9493: the deferred-write queue holds JS callbacks across turns.
            for write in &mut state.pending_writes {
                visitor.visit_nanbox_f64_slot(&mut write.callback);
            }
            visitor.visit_nanbox_f64_slot(&mut state.end_callback);
            visitor.visit_nanbox_f64_slot(&mut state.error_value);
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
        // Both string representations; empty for anything that is not a
        // string (`extract_string_ptr` is heap-`STRING_TAG` only, #8122).
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        match crate::string::str_bytes_from_jsvalue(v, &mut scratch) {
            Some((ptr, len)) if !ptr.is_null() => {
                std::slice::from_raw_parts(ptr, len as usize).to_vec()
            }
            _ => Vec::new(),
        }
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
///
/// `handled_locally` — this stream's own registry delivered the event. An
/// `'error'` it handled is forwarded only to listeners node:stream actually
/// holds: that emitter throws an `'error'` nobody listens to (#9493 — the
/// deferred open's failure reaches here for real, where the old synchronous
/// replay never did), and "nobody" has to mean neither registry.
fn bridge_to_stream_listeners(id: usize, event: &str, args: &[f64], handled_locally: bool) {
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
    if event == "error"
        && handled_locally
        && !crate::node_stream::has_stream_listeners(object_value, event.as_bytes())
    {
        return;
    }
    crate::node_stream::emit_to_stream_listeners(object_value, event.as_bytes(), args);
}

fn emit_event0(id: usize, event: &str) {
    use crate::closure::js_closure_call0;
    let callbacks = callbacks_for_event(id, event);
    let handled_locally = !callbacks.is_empty();
    for cb in callbacks {
        let cb_ptr = extract_closure_ptr(cb);
        if !cb_ptr.is_null() {
            js_closure_call0(cb_ptr);
        }
    }
    bridge_to_stream_listeners(id, event, &[], handled_locally);
}

fn emit_event1(id: usize, event: &str, arg: f64) {
    use crate::closure::js_closure_call1;
    let callbacks = callbacks_for_event(id, event);
    let handled_locally = !callbacks.is_empty();
    for cb in callbacks {
        let cb_ptr = extract_closure_ptr(cb);
        if !cb_ptr.is_null() {
            js_closure_call1(cb_ptr, arg);
        }
    }
    bridge_to_stream_listeners(id, event, &[arg], handled_locally);
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

/// The stream's stored error as a JS value: the node-shaped value the deferred
/// open produced when there is one (#9493), else an `Error` over `error_msg`.
fn stored_error_value(state: &StreamState) -> Option<f64> {
    if !JSValue::from_bits(state.error_value.to_bits()).is_undefined() {
        return Some(state.error_value);
    }
    state.error_msg.as_deref().map(make_error_value)
}

fn emit_stored_error(id: usize) {
    let error_value = STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        registry.get(&id).and_then(stored_error_value)
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

fn call_stream_callback0(callback: f64) {
    if is_callable_value(callback) {
        let cb_ptr = extract_closure_ptr(callback);
        if !cb_ptr.is_null() {
            crate::closure::js_closure_call0(cb_ptr);
        }
    }
}

fn call_stream_callback1(callback: f64, arg: f64) {
    if is_callable_value(callback) {
        let cb_ptr = extract_closure_ptr(callback);
        if !cb_ptr.is_null() {
            crate::closure::js_closure_call1(cb_ptr, arg);
        }
    }
}

/// #9493: park one `WriteStream` turn on the callback-timer queue. At most one
/// is pending per stream; a turn re-schedules while work remains.
///
/// This is the mechanism `fs::deferred` uses for `fs.writeFile`: the timer
/// queue roots the closure; a pending refed callback timer is a live event
/// source, so a program that ends by draining its loop still lands every byte
/// and `'finish'` still fires; and `process.exit()` terminates through
/// `libc::_exit` without ticking the queue, so an exit in the same tick
/// abandons the parked open and writes the way Node abandons its not-yet-run
/// thread-pool requests.
fn schedule_write_stream_turn(id: usize) {
    let should_schedule = STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(state) = registry.get_mut(&id) else {
            return false;
        };
        if state.turn_pending || state.closed {
            return false;
        }
        state.turn_pending = true;
        true
    });
    if should_schedule {
        let closure = js_closure_alloc(write_stream_turn_impl as *const u8, 1);
        js_closure_set_capture_ptr(closure, 0, id as i64);
        let _ = crate::timer::js_set_timeout_callback(closure as i64, 0.0);
    }
}

extern "C" fn write_stream_turn_impl(closure: *const ClosureHeader) -> f64 {
    let id = stream_id_of(closure);
    STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.turn_pending = false;
        }
    });
    run_write_stream_turn(id);
    undefined_value()
}

/// What a `WriteStream` turn does, decided from the state when it runs. One
/// step per turn, each the analogue of one Node thread-pool request: the open
/// (`_construct` → `fs.open`), then the queued writes ending in `'finish'`,
/// then the close. A step schedules the next when more work remains, so a
/// microtask queued by an `'open'` or `'finish'` listener runs before the
/// first write callback / before `'close'`, as it does in Node.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WriteStreamStep {
    Open,
    Drain,
    Close,
    Idle,
}

fn write_stream_step(id: usize) -> WriteStreamStep {
    STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let Some(state) = registry.get(&id) else {
            return WriteStreamStep::Idle;
        };
        if state.kind != StreamKind::Write || state.closed {
            return WriteStreamStep::Idle;
        }
        if state.destroyed {
            return WriteStreamStep::Close;
        }
        if !state.opened && state.error_msg.is_none() && matches!(state.owner, FdOwner::Path) {
            return WriteStreamStep::Open;
        }
        if !state.pending_writes.is_empty() || (state.ended && !state.finished) {
            return WriteStreamStep::Drain;
        }
        if state.finished && state.auto_close {
            return WriteStreamStep::Close;
        }
        WriteStreamStep::Idle
    })
}

fn run_write_stream_turn(id: usize) {
    match write_stream_step(id) {
        WriteStreamStep::Open => write_stream_open_step(id),
        WriteStreamStep::Drain => write_stream_drain_step(id),
        WriteStreamStep::Close => write_stream_close_step(id),
        WriteStreamStep::Idle => {}
    }
}

fn schedule_next_write_stream_step(id: usize) {
    if write_stream_step(id) != WriteStreamStep::Idle {
        schedule_write_stream_turn(id);
    }
}

/// The deferred `open(2)`: Node's `_construct` runs `fs.open` on the pool and
/// then emits `'open'` and `'ready'`. The queued writes are performed on a
/// LATER turn — their `fs.write` requests are only dispatched once the open
/// callback has returned.
fn write_stream_open_step(id: usize) {
    let (path, flags) = STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .map(|state| (state.path.clone(), state.flags.clone()))
            .unwrap_or_default()
    });
    match fs_open_path_str_result(&path, &flags) {
        Ok(fd) => {
            STREAM_REGISTRY.with(|registry| {
                if let Some(state) = registry.borrow_mut().get_mut(&id) {
                    state.fd = Some(fd);
                    state.opened = true;
                    if matches!(state.flags.as_str(), "a" | "a+" | "ax" | "ax+") {
                        state.position = end_position_for_fd(fd);
                    }
                    update_common_props(state);
                }
            });
            emit_event1(id, "open", fd as f64);
            emit_event0(id, "ready");
            schedule_next_write_stream_step(id);
        }
        Err(err) => {
            let message = err.to_string();
            let error_value = unsafe { build_fs_error_value(&err, "open", &path) };
            write_stream_fail(id, error_value, message);
        }
    }
}

/// The error cascade Node runs when the open fails: every pending write
/// callback and the `end()` callback receive the error, then `'error'` fires,
/// then the stream is destroyed and `'close'` follows on a later turn.
fn write_stream_fail(id: usize, error_value: f64, message: String) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let error_handle = scope.root_nanbox_f64(error_value);
    // The value goes into the state first — the registry is a GC root and the
    // callbacks below allocate.
    let (writes, end_callback) = STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(state) = registry.get_mut(&id) else {
            return (Vec::new(), undefined_value());
        };
        state.errored = true;
        state.error_msg = Some(message);
        state.error_value = error_handle.get_nanbox_f64();
        state.destroyed = true;
        state.writable_length = 0;
        state.writable_need_drain = false;
        let writes = std::mem::take(&mut state.pending_writes);
        let end_callback = std::mem::replace(&mut state.end_callback, undefined_value());
        update_common_props(state);
        (writes, end_callback)
    });
    let callbacks: Vec<_> = writes
        .iter()
        .map(|write| scope.root_nanbox_f64(write.callback))
        .collect();
    let end_handle = scope.root_nanbox_f64(end_callback);
    for callback in &callbacks {
        call_stream_callback1(callback.get_nanbox_f64(), error_handle.get_nanbox_f64());
    }
    call_stream_callback1(end_handle.get_nanbox_f64(), error_handle.get_nanbox_f64());
    emit_event1(id, "error", error_handle.get_nanbox_f64());
    schedule_next_write_stream_step(id);
}

/// The queued writes, in order — Node batches them into one `writev` — then
/// Node's `afterWrite` order: `'drain'` (when a `write()` returned `false`
/// and the stream is not ending) BEFORE the completed writes' callbacks. Once
/// `end()` has been called and nothing is left: the `end()` callback and
/// `'finish'`; with `autoClose`, the close lands on the next turn.
fn write_stream_drain_step(id: usize) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let mut completed = Vec::new();
    loop {
        let next = STREAM_REGISTRY.with(|registry| {
            let mut registry = registry.borrow_mut();
            let state = registry.get_mut(&id)?;
            if state.destroyed || state.pending_writes.is_empty() {
                return None;
            }
            Some(state.pending_writes.remove(0))
        });
        let Some(write) = next else {
            break;
        };
        let callback = scope.root_nanbox_f64(write.callback);
        match write_to_stream_fd(id, &write.bytes) {
            Ok(()) => completed.push(callback),
            Err(message) => {
                // The writes that did land complete normally; the failing one
                // gets the error, then the rest of the queue does via the
                // error cascade.
                for done in &completed {
                    call_stream_callback0(done.get_nanbox_f64());
                }
                let error_value = scope.root_nanbox_f64(make_error_value(&message));
                call_stream_callback1(callback.get_nanbox_f64(), error_value.get_nanbox_f64());
                write_stream_fail(id, error_value.get_nanbox_f64(), message);
                return;
            }
        }
    }
    let (emit_drain, finish) = STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(state) = registry.get_mut(&id) else {
            return (false, false);
        };
        if state.destroyed {
            return (false, false);
        }
        state.writable_length = 0;
        let emit_drain = state.writable_need_drain && !state.ended;
        state.writable_need_drain = false;
        let mut finish = false;
        if state.ended && !state.finished {
            if state.error_msg.is_none() {
                state.finished = true;
                finish = true;
            } else {
                state.destroyed = true;
            }
        }
        update_common_props(state);
        (emit_drain, finish)
    });
    if emit_drain {
        emit_event0(id, "drain");
    }
    for callback in &completed {
        call_stream_callback0(callback.get_nanbox_f64());
    }
    if finish {
        let end_callback = STREAM_REGISTRY.with(|registry| {
            registry
                .borrow_mut()
                .get_mut(&id)
                .map(|state| std::mem::replace(&mut state.end_callback, undefined_value()))
                .unwrap_or_else(undefined_value)
        });
        let scope = crate::gc::RuntimeHandleScope::new();
        let end_callback = scope.root_nanbox_f64(end_callback);
        call_stream_callback0(end_callback.get_nanbox_f64());
        emit_event0(id, "finish");
    }
    schedule_next_write_stream_step(id);
}

fn write_stream_close_step(id: usize) {
    let force = STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .map(|state| state.destroyed)
            .unwrap_or(false)
    });
    maybe_close_stream(id, force);
}

pub(crate) extern "C" fn write_stream_write_impl(
    closure: *const ClosureHeader,
    chunk: f64,
    encoding: f64,
    cb: f64,
) -> f64 {
    let id = stream_id_of(closure);
    let (chunk_value, callback) = normalize_write_args(chunk, encoding, cb);
    let Some(chunk_value) = chunk_value else {
        call_stream_callback0(callback.unwrap_or_else(undefined_value));
        return bool_value(true);
    };
    // Decoding the chunk can allocate; the callback outlives that.
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback = scope.root_nanbox_f64(callback.unwrap_or_else(undefined_value));
    let bytes = bytes_from_value(chunk_value);
    // #9493: accept the chunk into the queue and answer the back-pressure
    // question from the queued length, as Node does; the write itself runs on
    // a later turn.
    let accepted = STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let state = registry.get_mut(&id)?;
        if state.kind != StreamKind::Write || state.ended || state.destroyed || state.closed {
            return None;
        }
        state.writable_length = state.writable_length.saturating_add(bytes.len());
        let over_hwm = state.writable_length >= state.high_water_mark;
        if over_hwm {
            state.writable_need_drain = true;
        }
        state.pending_writes.push(PendingWrite {
            bytes,
            callback: callback.get_nanbox_f64(),
        });
        update_common_props(state);
        Some(!over_hwm)
    });
    let Some(below_hwm) = accepted else {
        return bool_value(false);
    };
    schedule_write_stream_turn(id);
    bool_value(below_hwm)
}

pub(crate) extern "C" fn write_stream_end_impl(
    closure: *const ClosureHeader,
    chunk: f64,
    encoding: f64,
    cb: f64,
) -> f64 {
    let id = stream_id_of(closure);
    let (chunk_value, callback) = normalize_write_args(chunk, encoding, cb);
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback = scope.root_nanbox_f64(callback.unwrap_or_else(undefined_value));
    let bytes = chunk_value.map(bytes_from_value);
    let accepted = STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(state) = registry.get_mut(&id) else {
            return false;
        };
        if state.kind != StreamKind::Write || state.ended || state.destroyed || state.closed {
            return false;
        }
        if let Some(bytes) = bytes {
            if !bytes.is_empty() {
                state.writable_length = state.writable_length.saturating_add(bytes.len());
                state.pending_writes.push(PendingWrite {
                    bytes,
                    callback: undefined_value(),
                });
            }
        }
        state.ended = true;
        state.end_callback = callback.get_nanbox_f64();
        update_common_props(state);
        true
    });
    if accepted {
        // #9493: `'finish'` and the callback land once the queue has drained,
        // on a later turn — `writableEnded` flips now, `writableFinished`
        // then, as in Node.
        schedule_write_stream_turn(id);
    } else {
        emit_stored_error(id);
    }
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

mod options_init;
use options_init::*;
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
            state.pending_writes.clear();
            state.writable_length = 0;
            state.writable_need_drain = false;
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
    if emit_pending_read_error(id) {
        return;
    }

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
            // A read stream still opens eagerly, so its `'open'`/`'ready'` are
            // replayed to the listeners attached right after construction. A
            // write stream opens on a later turn (#9493) and emits them for
            // real; Node never fires either for a supplied fd.
            "open"
                if state.kind == StreamKind::Read
                    && state.opened
                    && !matches!(state.owner, FdOwner::External | FdOwner::FileHandle(_)) =>
            {
                state.fd.map(|fd| ("open", fd as f64))
            }
            "ready" if state.kind == StreamKind::Read && state.opened => {
                Some(("ready", undefined_value()))
            }
            "error" => stored_error_value(state).map(|err| ("error", err)),
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
    // #9493: a path-owned stream opens on the next turn. Scheduling allocates
    // the turn closure, so the object is re-read from its rooted slot rather
    // than from the local that created it.
    schedule_next_write_stream_step(id);
    STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .map(|state| state.object_value)
            .unwrap_or(value)
    })
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
