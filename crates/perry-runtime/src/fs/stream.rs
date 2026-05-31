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

fn write_file_data_type_error(value: f64) -> f64 {
    let message = format!(
        "The \"data\" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::build_type_error_with_code_value(&message, "ERR_INVALID_ARG_TYPE")
}

fn write_file_signal_type_error(value: f64) -> f64 {
    let message = format!(
        "The \"options.signal\" property must be an instance of AbortSignal. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::build_type_error_with_code_value(&message, "ERR_INVALID_ARG_TYPE")
}

fn write_file_signal_from_options(options_value: f64) -> Result<Option<*mut ObjectHeader>, f64> {
    let value = JSValue::from_bits(options_value.to_bits());
    if value.is_undefined() || value.is_null() || value.is_any_string() {
        return Ok(None);
    }
    unsafe {
        let Some(signal_value) = options_field_value(options_value, b"signal") else {
            return Ok(None);
        };
        let signal_f64 = f64::from_bits(signal_value.bits());
        let signal_js = JSValue::from_bits(signal_value.bits());
        if signal_js.is_undefined() {
            return Ok(None);
        }
        match crate::url::abort::abort_signal_ptr_from_value(signal_f64) {
            Some(signal) => Ok(Some(signal)),
            None => Err(write_file_signal_type_error(signal_f64)),
        }
    }
}

fn check_write_file_aborted(signal: Option<*mut ObjectHeader>) -> Result<(), f64> {
    let Some(signal) = signal else {
        return Ok(());
    };
    if crate::url::js_abort_signal_is_aborted(signal) != 0 {
        Err(crate::url::js_abort_error_value())
    } else {
        Ok(())
    }
}

fn write_file_chunk_bytes(value: f64, encoding_tag: i32) -> Result<Vec<u8>, f64> {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_any_string() {
        return Ok(bytes_from_string_value(value, encoding_tag));
    }
    if crate::buffer::js_buffer_is_buffer(value.to_bits() as i64) == 1 {
        return Ok(bytes_from_buffer_value(value));
    }
    let bits = value.to_bits();
    let addr = if (bits >> 48) >= 0x7FF8 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        bits as usize
    };
    if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
        let ta = addr as *const crate::typedarray::TypedArrayHeader;
        if let Some(bytes) = unsafe { crate::typedarray::typed_array_bytes(ta) } {
            return Ok(bytes.to_vec());
        }
        return Ok(Vec::new());
    }
    if crate::array::js_array_is_array(value).to_bits() == crate::value::TAG_TRUE {
        let buf = crate::buffer::js_buffer_from_value(value.to_bits() as i64, encoding_tag);
        if buf.is_null() {
            return Ok(Vec::new());
        }
        return Ok(unsafe {
            std::slice::from_raw_parts(crate::buffer::buffer_data(buf), (*buf).length as usize)
                .to_vec()
        });
    }
    Err(write_file_data_type_error(value))
}

fn write_file_raw_ptr_from_value(value: f64) -> usize {
    let bits = value.to_bits();
    let js = JSValue::from_bits(bits);
    if js.is_pointer() || js.is_string() || js.is_bigint() {
        return (bits & crate::value::POINTER_MASK) as usize;
    }
    if bits != 0 && bits < 0x0001_0000_0000_0000 {
        return bits as usize;
    }
    0
}

unsafe fn write_file_gc_type_for_ptr(raw: usize) -> Option<u8> {
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let header = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    let gc_type = (*header).obj_type;
    if gc_type <= crate::gc::GC_TYPE_MAX {
        Some(gc_type)
    } else {
        None
    }
}

fn write_file_object_ptr_from_value(value: f64) -> Option<*const ObjectHeader> {
    let raw = write_file_raw_ptr_from_value(value);
    if raw < 0x10000 || crate::buffer::is_registered_buffer(raw) {
        return None;
    }
    unsafe {
        if write_file_gc_type_for_ptr(raw) != Some(crate::gc::GC_TYPE_OBJECT) {
            return None;
        }
    }
    Some(raw as *const ObjectHeader)
}

fn is_callable_write_value(value: f64) -> bool {
    let raw = write_file_raw_ptr_from_value(value);
    raw >= 0x10000
        && !crate::buffer::is_registered_buffer(raw)
        && crate::closure::is_closure_ptr(raw)
}

fn well_known_iterator_method(value: f64, name: &str) -> Option<f64> {
    let sym = crate::symbol::well_known_symbol(name);
    if sym.is_null() {
        return None;
    }
    let sym_value = f64::from_bits(JSValue::pointer(sym as *const u8).bits());
    let method = unsafe { crate::symbol::js_object_get_symbol_property(value, sym_value) };
    if !is_callable_write_value(method) {
        return None;
    }
    Some(method)
}

fn call_well_known_iterator(value: f64, name: &str) -> Option<f64> {
    let method = well_known_iterator_method(value, name)?;
    let prev_this = crate::object::js_implicit_this_set(value);
    let iterator = unsafe { crate::closure::js_native_call_value(method, std::ptr::null(), 0) };
    crate::object::js_implicit_this_set(prev_this);
    if iterator.to_bits() == crate::value::TAG_UNDEFINED {
        None
    } else {
        Some(iterator)
    }
}

fn value_has_named_next(value: f64) -> bool {
    let Some(obj) = write_file_object_ptr_from_value(value) else {
        return false;
    };
    let key = js_string_from_bytes(b"next".as_ptr(), 4);
    let field = crate::object::js_object_get_field_by_name(obj, key);
    let field_value = f64::from_bits(field.bits());
    is_callable_write_value(field_value)
}

fn write_file_iterator_for_value(value: f64) -> Option<f64> {
    if write_file_raw_ptr_from_value(value) == 0 {
        return None;
    }
    if let Some(iter) = call_well_known_iterator(value, "asyncIterator") {
        return Some(iter);
    }
    if value_has_named_next(value) {
        return Some(value);
    }
    call_well_known_iterator(value, "iterator")
}

fn write_file_data_has_source(value: f64) -> bool {
    if is_direct_write_data(value) || fs_read_stream_id(value).is_some() {
        return true;
    }
    if write_file_raw_ptr_from_value(value) == 0 {
        return false;
    }
    well_known_iterator_method(value, "asyncIterator").is_some()
        || value_has_named_next(value)
        || well_known_iterator_method(value, "iterator").is_some()
}

fn validate_write_file_data_source(value: f64) -> Result<(), f64> {
    if write_file_data_has_source(value) {
        Ok(())
    } else {
        Err(write_file_data_type_error(value))
    }
}

fn settle_write_file_value(value: f64) -> Result<f64, f64> {
    if crate::promise::js_value_is_promise(value) == 0 {
        return Ok(value);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    for _ in 0..10_000 {
        let current = value_handle.get_nanbox_f64();
        if crate::promise::js_value_is_promise(current) == 0 {
            return Ok(current);
        }
        let promise = crate::value::js_nanbox_get_pointer(current) as *mut crate::promise::Promise;
        if promise.is_null() {
            return Ok(current);
        }
        unsafe {
            match (*promise).state {
                crate::promise::PromiseState::Fulfilled => return Ok((*promise).value),
                crate::promise::PromiseState::Rejected => return Err((*promise).reason),
                crate::promise::PromiseState::Pending => {}
            }
        }
        crate::event_pump::perry_poll();
        let _ = crate::timer::js_timer_tick();
        let _ = crate::timer::js_callback_timer_tick();
        let _ = crate::timer::js_interval_timer_tick();
        if crate::event_pump::perry_has_work() == 0 {
            break;
        }
        crate::event_pump::js_wait_for_event();
    }
    let current = value_handle.get_nanbox_f64();
    let promise = crate::value::js_nanbox_get_pointer(current) as *mut crate::promise::Promise;
    if promise.is_null() {
        return Ok(current);
    }
    unsafe {
        match (*promise).state {
            crate::promise::PromiseState::Fulfilled => Ok((*promise).value),
            crate::promise::PromiseState::Rejected => Err((*promise).reason),
            crate::promise::PromiseState::Pending => Ok(current),
        }
    }
}

fn iterator_result_value_done(result: f64) -> Option<(f64, bool)> {
    let obj = write_file_object_ptr_from_value(result)?;
    let done_key = js_string_from_bytes(b"done".as_ptr(), 4);
    let value_key = js_string_from_bytes(b"value".as_ptr(), 5);
    let done = crate::object::js_object_get_field_by_name(obj, done_key);
    let value = crate::object::js_object_get_field_by_name(obj, value_key);
    let done_f64 = f64::from_bits(done.bits());
    let value_f64 = f64::from_bits(value.bits());
    Some((value_f64, crate::value::js_is_truthy(done_f64) != 0))
}

fn consume_iterator_for_write_file<F>(
    iterator: f64,
    encoding_tag: i32,
    signal: Option<*mut ObjectHeader>,
    mut write_chunk: F,
) -> Result<(), f64>
where
    F: FnMut(&[u8]) -> Result<(), f64>,
{
    for _ in 0..100_000 {
        check_write_file_aborted(signal)?;
        let next_result = unsafe {
            crate::object::js_native_call_method(
                iterator,
                b"next".as_ptr() as *const i8,
                4,
                std::ptr::null(),
                0,
            )
        };
        let next_result = settle_write_file_value(next_result)?;
        let Some((chunk, done)) = iterator_result_value_done(next_result) else {
            return Ok(());
        };
        if done {
            return Ok(());
        }
        check_write_file_aborted(signal)?;
        let bytes = write_file_chunk_bytes(chunk, encoding_tag)?;
        write_chunk(&bytes)?;
    }
    Ok(())
}

fn fs_read_stream_id(value: f64) -> Option<usize> {
    STREAM_REGISTRY.with(|registry| {
        registry.borrow().iter().find_map(|(id, state)| {
            (state.kind == StreamKind::Read
                && state.object_value.to_bits() == value.to_bits()
                && !state.destroyed)
                .then_some(*id)
        })
    })
}

fn consume_fs_read_stream_for_write_file<F>(
    id: usize,
    signal: Option<*mut ObjectHeader>,
    mut write_chunk: F,
) -> Result<(), f64>
where
    F: FnMut(&[u8]) -> Result<(), f64>,
{
    loop {
        check_write_file_aborted(signal)?;
        match read_next_chunk(id) {
            Ok(Some((bytes, _encoding))) => {
                check_write_file_aborted(signal)?;
                write_chunk(&bytes)?;
            }
            Ok(None) => {
                finish_read_stream(id);
                return Ok(());
            }
            Err(message) => return Err(make_error_value(&message)),
        }
    }
}

fn consume_write_file_input<F>(data: f64, options: f64, mut write_chunk: F) -> Result<(), f64>
where
    F: FnMut(&[u8]) -> Result<(), f64>,
{
    let signal = write_file_signal_from_options(options)?;
    check_write_file_aborted(signal)?;
    let encoding_tag = encoding_tag_from_options(options);
    if is_direct_write_data(data) {
        let bytes = write_file_chunk_bytes(data, encoding_tag)?;
        return write_chunk(&bytes);
    }
    if let Some(id) = fs_read_stream_id(data) {
        return consume_fs_read_stream_for_write_file(id, signal, write_chunk);
    }
    if let Some(iterator) = write_file_iterator_for_value(data) {
        return consume_iterator_for_write_file(iterator, encoding_tag, signal, write_chunk);
    }
    Err(write_file_data_type_error(data))
}

fn write_fd_chunk_result(fd: i32, bytes: &[u8], force_append: bool) -> Result<(), f64> {
    let result = FD_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(file) = registry.get_mut(&fd) else {
            return Err(std::io::Error::from_raw_os_error(libc::EBADF));
        };
        if force_append
            || FD_APPEND_MODE.with(|flags| flags.borrow().get(&fd).copied().unwrap_or(false))
        {
            file.seek(SeekFrom::End(0))?;
        }
        file.write_all(bytes)
    });
    result.map_err(|err| unsafe { build_fs_error_value_no_path(&err, "write") })
}

pub(crate) unsafe fn write_file_to_fd_result(
    fd: i32,
    data: f64,
    options: f64,
    force_append: bool,
) -> Result<(), f64> {
    validate::validate_string_or_object_options("options", options);
    validate_write_file_data_source(data)?;
    consume_write_file_input(data, options, |bytes| {
        write_fd_chunk_result(fd, bytes, force_append)
    })
}

pub(crate) unsafe fn write_file_path_or_fd_result(
    path_value: f64,
    data: f64,
    options: f64,
) -> Result<(), f64> {
    validate::validate_path_or_fd("path", path_value, "write");
    validate::validate_string_or_object_options("options", options);
    validate_write_file_data_source(data)?;
    let signal = write_file_signal_from_options(options)?;
    check_write_file_aborted(signal)?;

    if let Some(fd) = numeric_fd_value(path_value) {
        return consume_write_file_input(data, options, |bytes| {
            write_fd_chunk_result(fd, bytes, false)
        });
    }

    let path = match decode_path_value(path_value) {
        Some(path) => path,
        None => validate::throw_invalid_path_arg("path", path_value),
    };
    let flag = file_options_flag(options, "w");
    let mut file = match open_file_for_write_flag(&path, &flag) {
        Ok(file) => file,
        Err(err) => return Err(build_fs_error_value(&err, "open", &path)),
    };
    consume_write_file_input(data, options, |bytes| {
        file.write_all(bytes)
            .map_err(|err| build_fs_error_value(&err, "write", &path))
    })
}

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

fn emit_event0(id: usize, event: &str) {
    use crate::closure::js_closure_call0;
    let callbacks = callbacks_for_event(id, event);
    for cb in callbacks {
        let cb_ptr = extract_closure_ptr(cb);
        if !cb_ptr.is_null() {
            js_closure_call0(cb_ptr);
        }
    }
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
    let should_emit_close = STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(state) = registry.get_mut(&id) else {
            return false;
        };
        if state.closed {
            return false;
        }
        if !force && !state.auto_close {
            return false;
        }
        close_fd_for_state(state);
        update_common_props(state);
        state.emit_close
    });
    if should_emit_close {
        emit_event0(id, "close");
    }
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

fn throw_utf8_invalid_arg_type(name: &str, expected: &str, value: f64) -> ! {
    let message = format!(
        "The \"{}\" argument must be of type {}. Received {}",
        name,
        expected,
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn throw_utf8_invalid_arg_value(name: &str, value: impl std::fmt::Display, reason: &str) -> ! {
    let message = format!(
        "The argument '{}' is invalid. Received {}. {}",
        name, value, reason
    );
    crate::fs::validate::throw_range_error_named(&message, "ERR_INVALID_ARG_VALUE")
}

fn js_number_value(value: f64) -> Option<f64> {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_int32() {
        Some(js.as_int32() as f64)
    } else if js.is_number() || value.is_finite() {
        Some(value)
    } else {
        None
    }
}

fn js_i32_number_value(value: f64) -> Option<i32> {
    let n = js_number_value(value)?;
    if n.is_finite() && n.fract() == 0.0 && n >= 0.0 && n <= i32::MAX as f64 {
        Some(n as i32)
    } else {
        None
    }
}

fn is_undefined_or_null(value: f64) -> bool {
    let js = JSValue::from_bits(value.to_bits());
    js.is_undefined() || js.is_null()
}

fn is_undefined_value(value: f64) -> bool {
    JSValue::from_bits(value.to_bits()).is_undefined()
}

fn option_raw_field(options_value: f64, field: &[u8]) -> Option<f64> {
    unsafe { options_field_value(options_value, field).map(|v| f64::from_bits(v.bits())) }
}

fn object_field_value_by_name(object_value: f64, name: &[u8]) -> Option<f64> {
    let obj = object_ptr_from_value(object_value)?;
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = crate::object::js_object_get_field_by_name(obj, key);
    Some(f64::from_bits(value.bits()))
}

fn object_has_callable_field(object_value: f64, name: &[u8]) -> bool {
    object_field_value_by_name(object_value, name)
        .map(is_callable_value)
        .unwrap_or(false)
}

fn validate_utf8_options_object(options_value: f64) {
    let js = JSValue::from_bits(options_value.to_bits());
    if js.is_undefined() || object_ptr_from_value(options_value).is_some() {
        return;
    }
    let message = format!(
        "The \"options\" argument must be of type object. Received {}",
        crate::fs::validate::describe_received(options_value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

fn validate_utf8_custom_fs(options_value: f64) -> f64 {
    let Some(fs_value) = option_raw_field(options_value, b"fs") else {
        return undefined_value();
    };
    if is_undefined_value(fs_value) {
        return undefined_value();
    }
    if object_ptr_from_value(fs_value).is_none() {
        let message = format!(
            "The \"options.fs\" argument must be of type object. Received {}",
            crate::fs::validate::describe_received(fs_value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    for method in [
        b"write".as_slice(),
        b"writeSync".as_slice(),
        b"fsync".as_slice(),
        b"fsyncSync".as_slice(),
        b"close".as_slice(),
        b"open".as_slice(),
        b"mkdir".as_slice(),
        b"mkdirSync".as_slice(),
    ] {
        if let Some(value) = object_field_value_by_name(fs_value, method) {
            if !is_undefined_value(value) && !is_callable_value(value) {
                let method_name = String::from_utf8_lossy(method);
                let message = format!(
                    "The \"options.fs.{}\" argument must be of type function. Received {}",
                    method_name,
                    crate::fs::validate::describe_received(value)
                );
                crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
            }
        }
    }
    fs_value
}

fn utf8_option_bool(options_value: f64, field: &[u8], default_value: bool) -> bool {
    let Some(value) = option_raw_field(options_value, field) else {
        return default_value;
    };
    if is_undefined_value(value) {
        return default_value;
    }
    if crate::value::js_is_truthy(value) == 0 {
        return false;
    }
    let js = JSValue::from_bits(value.to_bits());
    if js.is_bool() {
        return true;
    }
    let field_name = format!("options.{}", String::from_utf8_lossy(field));
    let message = format!(
        "The \"{}\" argument must be of type boolean. Received {}",
        field_name,
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn utf8_option_uint32(options_value: f64, field: &[u8], default_value: usize) -> usize {
    let Some(value) = option_raw_field(options_value, field) else {
        return default_value;
    };
    if is_undefined_value(value) || crate::value::js_is_truthy(value) == 0 {
        return default_value;
    }
    let Some(n) = js_number_value(value) else {
        let field_name = format!("options.{}", String::from_utf8_lossy(field));
        let message = format!(
            "The \"{}\" argument must be of type number. Received {}",
            field_name,
            crate::fs::validate::describe_received(value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    };
    if !n.is_finite() || n.fract() != 0.0 || n < 0.0 || n > u32::MAX as f64 {
        let field_name = format!("options.{}", String::from_utf8_lossy(field));
        let message = format!(
            "The value of \"{}\" is out of range. It must be >= 0 && <= {}. Received {}",
            field_name,
            u32::MAX,
            crate::fs::validate::format_received_number(n)
        );
        crate::fs::validate::throw_range_error_with_code(&message);
    }
    n as usize
}

fn utf8_content_mode(options_value: f64) -> Utf8ContentMode {
    let Some(value) = option_raw_field(options_value, b"contentMode") else {
        return Utf8ContentMode::Utf8;
    };
    if is_undefined_value(value) {
        return Utf8ContentMode::Utf8;
    }
    let Some(mode) = js_string_value(value) else {
        let message = format!(
            "The \"options.contentMode\" argument must be one of: 'buffer', 'utf8'. Received {}",
            crate::fs::validate::describe_received(value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    };
    match mode.as_str() {
        "utf8" => Utf8ContentMode::Utf8,
        "buffer" => Utf8ContentMode::Buffer,
        _ => {
            let message = format!(
                "The \"options.contentMode\" argument must be one of: 'buffer', 'utf8'. Received '{}'",
                mode
            );
            crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE")
        }
    }
}

fn utf8_retry_eagain(options_value: f64) -> f64 {
    let Some(value) = option_raw_field(options_value, b"retryEAGAIN") else {
        return undefined_value();
    };
    if is_undefined_value(value) || crate::value::js_is_truthy(value) == 0 {
        return undefined_value();
    }
    if is_callable_value(value) {
        return value;
    }
    let message = format!(
        "The \"options.retryEAGAIN\" argument must be of type function. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn utf8_fd_or_dest(options_value: f64) -> f64 {
    let fd = option_raw_field(options_value, b"fd").unwrap_or_else(undefined_value);
    if !is_undefined_or_null(fd) {
        return fd;
    }
    option_raw_field(options_value, b"dest").unwrap_or_else(undefined_value)
}

fn utf8_stream_parent_dir(file: &str) -> Option<&Path> {
    let parent = Path::new(file).parent()?;
    if parent.as_os_str().is_empty() {
        None
    } else {
        Some(parent)
    }
}

fn utf8_call_custom_method(custom_fs: f64, method: &[u8], args: &[f64]) -> Option<f64> {
    if is_undefined_value(custom_fs) || !object_has_callable_field(custom_fs, method) {
        return None;
    }
    Some(unsafe {
        crate::object::js_native_call_method(
            custom_fs,
            method.as_ptr() as *const i8,
            method.len(),
            args.as_ptr(),
            args.len(),
        )
    })
}

fn utf8_register_native_fd(file: std::fs::File, path: &str, append_mode: bool) -> i32 {
    let fd = NEXT_FD.with(|n| {
        let mut n = n.borrow_mut();
        let fd = *n;
        *n += 1;
        fd
    });
    FD_REGISTRY.with(|r| {
        r.borrow_mut().insert(fd, file);
    });
    FD_PATHS.with(|r| {
        r.borrow_mut().insert(fd, path.to_string());
    });
    FD_APPEND_MODE.with(|r| {
        r.borrow_mut().insert(fd, append_mode);
    });
    fd
}

fn utf8_open_path_result(
    path_value: f64,
    file: &str,
    append: bool,
    mkdir: bool,
    custom_fs: f64,
) -> Result<i32, f64> {
    if mkdir {
        if let Some(parent) = utf8_stream_parent_dir(file) {
            if let Some(parent_str) = parent.to_str() {
                let parent_value = string_value_str(parent_str);
                let options_obj = crate::object::js_object_alloc(0, 1);
                let key = js_string_from_bytes(b"recursive".as_ptr(), 9);
                crate::object::js_object_set_field_by_name(options_obj, key, bool_value(true));
                let options_value = object_value(options_obj);
                if utf8_call_custom_method(custom_fs, b"mkdirSync", &[parent_value, options_value])
                    .is_none()
                {
                    let _ = std::fs::create_dir_all(parent);
                }
            }
        }
    }

    let flags = if append { "a" } else { "w" };
    let flags_value = string_value_str(flags);
    if let Some(value) = utf8_call_custom_method(
        custom_fs,
        b"openSync",
        &[path_value, flags_value, undefined_value()],
    ) {
        if let Some(fd) = js_i32_number_value(value) {
            return Ok(fd);
        }
    }
    match unsafe { fs_open_sync_result(path_value, flags_value) } {
        Ok(fd) => Ok(fd),
        Err((err, path)) => Err(unsafe { build_fs_error_value(&err, "open", &path) }),
    }
}

fn utf8_open_path(path_value: f64, file: &str, append: bool, mkdir: bool, custom_fs: f64) -> i32 {
    match utf8_open_path_result(path_value, file, append, mkdir, custom_fs) {
        Ok(fd) => fd,
        Err(err) => crate::exception::js_throw(err),
    }
}

fn utf8_native_mkdir_result(file: &str) -> Result<(), f64> {
    if let Some(parent) = utf8_stream_parent_dir(file) {
        if let Err(err) = std::fs::create_dir_all(parent) {
            let path = parent.to_string_lossy();
            return Err(unsafe { build_fs_error_value(&err, "mkdir", &path) });
        }
    }
    Ok(())
}

fn utf8_native_async_open_path_result(
    path_value: f64,
    file: &str,
    append: bool,
    mkdir: bool,
) -> Result<i32, f64> {
    validate::validate_path("path", path_value);
    if mkdir {
        utf8_native_mkdir_result(file)?;
    }

    let mut options = std::fs::OpenOptions::new();
    if append {
        options.write(true).create(true).append(true);
    } else {
        options.write(true).create(true).truncate(true);
    }
    match options.open(file) {
        Ok(file_handle) => Ok(utf8_register_native_fd(file_handle, file, append)),
        Err(err) => Err(unsafe { build_fs_error_value(&err, "open", file) }),
    }
}

fn utf8_initial_state(options_value: f64) -> Utf8StreamState {
    validate_utf8_options_object(options_value);
    let custom_fs = validate_utf8_custom_fs(options_value);
    let content_mode = utf8_content_mode(options_value);
    let min_length = utf8_option_uint32(options_value, b"minLength", 0);
    let max_length = utf8_option_uint32(options_value, b"maxLength", 0);
    let max_write = utf8_option_uint32(options_value, b"maxWrite", 16 * 1024);
    let periodic_flush = utf8_option_uint32(options_value, b"periodicFlush", 0);
    let sync = utf8_option_bool(options_value, b"sync", false);
    let fsync = utf8_option_bool(options_value, b"fsync", false);
    let append = utf8_option_bool(options_value, b"append", true);
    let mkdir = utf8_option_bool(options_value, b"mkdir", false);
    let retry_eagain = utf8_retry_eagain(options_value);
    if min_length >= max_write {
        throw_utf8_invalid_arg_value(
            "minLength",
            min_length,
            &format!("should be smaller than maxWrite ({})", max_write),
        );
    }
    let mode_value = option_raw_field(options_value, b"mode").unwrap_or_else(undefined_value);
    let fd_or_dest = utf8_fd_or_dest(options_value);
    let mut file = None;
    let mut pending_file = None;
    let mut opening = false;
    let mut writing = false;
    let fd = if let Some(numeric_fd) = js_i32_number_value(fd_or_dest) {
        numeric_fd
    } else if JSValue::from_bits(fd_or_dest.to_bits()).is_any_string() {
        let path = path_from_value(fd_or_dest);
        if sync {
            let opened_fd = utf8_open_path(fd_or_dest, &path, append, mkdir, custom_fs);
            file = Some(path);
            opened_fd
        } else {
            pending_file = Some(path);
            opening = true;
            writing = true;
            -1
        }
    } else {
        throw_utf8_invalid_arg_type("fd", "number or string", fd_or_dest);
    };
    Utf8StreamState {
        fd,
        file,
        pending_file,
        reopen_old_fd: None,
        append,
        content_mode,
        sync,
        fsync,
        min_length,
        max_length,
        max_write,
        periodic_flush,
        periodic_flush_timer: None,
        mkdir,
        mode_value,
        retry_eagain,
        custom_fs,
        buffers: Vec::new(),
        len: 0,
        writing,
        opening,
        ending: false,
        destroyed: false,
        closed: false,
        listeners: StdHashMap::new(),
        object_value: undefined_value(),
    }
}

fn utf8_content_mode_str(mode: Utf8ContentMode) -> &'static str {
    match mode {
        Utf8ContentMode::Utf8 => "utf8",
        Utf8ContentMode::Buffer => "buffer",
    }
}

fn update_utf8_props(state: &Utf8StreamState) {
    let obj = state.object_value;
    set_object_field(obj, b"append", bool_value(state.append));
    set_object_field_str(
        obj,
        b"contentMode",
        utf8_content_mode_str(state.content_mode),
    );
    set_object_field(obj, b"fd", state.fd as f64);
    match state.file.as_deref() {
        Some(file) => set_object_field_str(obj, b"file", file),
        None => set_object_field(obj, b"file", null_value()),
    }
    set_object_field(obj, b"fsync", bool_value(state.fsync));
    set_object_field(obj, b"maxLength", state.max_length as f64);
    set_object_field(obj, b"minLength", state.min_length as f64);
    set_object_field(obj, b"mkdir", bool_value(state.mkdir));
    set_object_field(obj, b"mode", state.mode_value);
    set_object_field(obj, b"periodicFlush", state.periodic_flush as f64);
    set_object_field(obj, b"sync", bool_value(state.sync));
    set_object_field(obj, b"writing", bool_value(state.writing));
    set_object_field(obj, b"destroyed", bool_value(state.destroyed));
}

fn utf8_callbacks_for_event(id: usize, event: &str) -> Vec<f64> {
    UTF8_STREAM_REGISTRY.with(|registry| {
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

fn utf8_emit_event0(id: usize, event: &str) {
    let callbacks = utf8_callbacks_for_event(id, event);
    for cb in callbacks {
        let cb_ptr = extract_closure_ptr(cb);
        if !cb_ptr.is_null() {
            crate::closure::js_closure_call0(cb_ptr);
        }
    }
}

fn utf8_emit_event1(id: usize, event: &str, arg: f64) {
    let callbacks = utf8_callbacks_for_event(id, event);
    for cb in callbacks {
        let cb_ptr = extract_closure_ptr(cb);
        if !cb_ptr.is_null() {
            crate::closure::js_closure_call1(cb_ptr, arg);
        }
    }
}

fn utf8_add_listener(id: usize, event: &str, cb: f64, once: bool) {
    if !is_callable_value(cb) {
        return;
    }
    let immediate = UTF8_STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let Some(state) = registry.get(&id) else {
            return None;
        };
        match event {
            "ready" if state.fd >= 0 && !state.destroyed => Some(("ready", undefined_value())),
            "finish" if state.ending && state.destroyed => Some(("finish", undefined_value())),
            "close" if state.closed => Some(("close", undefined_value())),
            _ => None,
        }
    });
    if let Some((_name, _arg)) = immediate {
        let cb_ptr = extract_closure_ptr(cb);
        if !cb_ptr.is_null() {
            crate::closure::js_closure_call0(cb_ptr);
        }
        return;
    }
    UTF8_STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state
                .listeners
                .entry(event.to_string())
                .or_default()
                .push(StreamListener { callback: cb, once });
        }
    });
}

fn utf8_remove_listener(id: usize, event: &str, cb: f64) {
    UTF8_STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            if let Some(listeners) = state.listeners.get_mut(event) {
                listeners.retain(|listener| listener.callback.to_bits() != cb.to_bits());
            }
        }
    });
}

fn utf8_buffered_chunk_value(mode: Utf8ContentMode, bytes: &[u8]) -> f64 {
    match mode {
        Utf8ContentMode::Utf8 => string_value_str(&String::from_utf8_lossy(bytes)),
        Utf8ContentMode::Buffer => buffer_value_from_bytes(bytes),
    }
}

fn utf8_native_write_fd(fd: i32, bytes: &[u8]) -> Result<usize, f64> {
    if bytes.is_empty() {
        return Ok(0);
    }
    write_fd_chunk_result(fd, bytes, false).map(|_| bytes.len())
}

fn utf8_write_chunk(id: usize, bytes: &[u8]) -> Result<usize, f64> {
    let (fd, mode, custom_fs) = UTF8_STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let Some(state) = registry.get(&id) else {
            return (-1, Utf8ContentMode::Utf8, undefined_value());
        };
        (state.fd, state.content_mode, state.custom_fs)
    });
    if fd < 0 {
        return Err(crate::fs::validate::build_ebadf_error_value("write"));
    }
    if !is_undefined_value(custom_fs) && object_has_callable_field(custom_fs, b"writeSync") {
        let data = utf8_buffered_chunk_value(mode, bytes);
        let result = if mode == Utf8ContentMode::Utf8 {
            utf8_call_custom_method(
                custom_fs,
                b"writeSync",
                &[fd as f64, data, string_value(b"utf8")],
            )
        } else {
            utf8_call_custom_method(custom_fs, b"writeSync", &[fd as f64, data])
        };
        return Ok(result
            .and_then(js_number_value)
            .unwrap_or(bytes.len() as f64) as usize);
    }
    utf8_native_write_fd(fd, bytes)
}

fn utf8_fsync(id: usize) {
    let (fd, custom_fs) = UTF8_STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let Some(state) = registry.get(&id) else {
            return (-1, undefined_value());
        };
        (state.fd, state.custom_fs)
    });
    if fd < 0 {
        return;
    }
    if !is_undefined_value(custom_fs) && object_has_callable_field(custom_fs, b"fsyncSync") {
        let _ = utf8_call_custom_method(custom_fs, b"fsyncSync", &[fd as f64]);
    } else if fd_is_registered(fd) {
        let _ = fsync_sync_inner(fd);
    }
}

fn utf8_close_fd(id: usize) {
    let (fd, custom_fs, timer) = UTF8_STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(state) = registry.get_mut(&id) else {
            return (-1, undefined_value(), None);
        };
        let timer = state.periodic_flush_timer.take();
        (state.fd, state.custom_fs, timer)
    });
    if let Some(timer) = timer {
        crate::timer::clearInterval(timer);
    }
    if fd >= 0 {
        if !is_undefined_value(custom_fs) && object_has_callable_field(custom_fs, b"closeSync") {
            let _ = utf8_call_custom_method(custom_fs, b"closeSync", &[fd as f64]);
        } else if fd_is_registered(fd) {
            let _ = js_fs_close_sync(fd as f64);
        }
    }
    UTF8_STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.closed = true;
            update_utf8_props(state);
        }
    });
}

fn utf8_drain_buffers(id: usize) -> Result<(), f64> {
    loop {
        let next = UTF8_STREAM_REGISTRY.with(|registry| {
            let mut registry = registry.borrow_mut();
            let Some(state) = registry.get_mut(&id) else {
                return None;
            };
            if state.destroyed || state.buffers.is_empty() {
                return None;
            }
            state.writing = true;
            update_utf8_props(state);
            Some(state.buffers.remove(0))
        });
        let Some(bytes) = next else {
            break;
        };
        let written = utf8_write_chunk(id, &bytes)?;
        UTF8_STREAM_REGISTRY.with(|registry| {
            if let Some(state) = registry.borrow_mut().get_mut(&id) {
                state.len = state.len.saturating_sub(written);
                update_utf8_props(state);
            }
        });
        utf8_emit_event1(id, "write", written as f64);
    }
    let should_fsync = UTF8_STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(state) = registry.get_mut(&id) else {
            return false;
        };
        state.writing = false;
        update_utf8_props(state);
        state.fsync
    });
    if should_fsync {
        utf8_fsync(id);
    }
    Ok(())
}

fn utf8_maybe_drain_after_write(id: usize) {
    let should_write = UTF8_STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let Some(state) = registry.get(&id) else {
            return false;
        };
        !state.writing && state.len >= state.min_length
    });
    if should_write {
        match utf8_drain_buffers(id) {
            Ok(()) => utf8_emit_event0(id, "drain"),
            Err(err) => utf8_emit_event1(id, "error", err),
        }
    }
}

fn utf8_stream_id_from_value(stream_value: f64) -> Option<usize> {
    let bits = stream_value.to_bits();
    UTF8_STREAM_REGISTRY.with(|registry| {
        registry.borrow().iter().find_map(|(id, state)| {
            if state.object_value.to_bits() == bits {
                Some(*id)
            } else {
                None
            }
        })
    })
}

fn utf8_stream_write_by_id(id: usize, data: f64) -> f64 {
    let (destroyed, mode, max_length, max_write, high_water_mark, current_len) =
        UTF8_STREAM_REGISTRY.with(|registry| {
            let registry = registry.borrow();
            let Some(state) = registry.get(&id) else {
                return (true, Utf8ContentMode::Utf8, 0, 16 * 1024, 16 * 1024 + 3, 0);
            };
            (
                state.destroyed,
                state.content_mode,
                state.max_length,
                state.max_write,
                state.min_length.max(16 * 1024 + 3),
                state.len,
            )
        });
    if destroyed {
        crate::fs::validate::throw_error_with_code(
            "Invalid state: Utf8Stream is destroyed",
            "ERR_INVALID_STATE",
        );
    }
    let bytes = match mode {
        Utf8ContentMode::Utf8 => {
            if !JSValue::from_bits(data.to_bits()).is_any_string() {
                throw_utf8_invalid_arg_type("data", "string", data);
            }
            bytes_from_value(data)
        }
        Utf8ContentMode::Buffer => {
            if crate::buffer::js_buffer_is_buffer(data.to_bits() as i64) != 1 {
                throw_utf8_invalid_arg_type("data", "Buffer", data);
            }
            bytes_from_buffer_value(data)
        }
    };
    let new_len = current_len.saturating_add(bytes.len());
    if max_length > 0 && new_len > max_length {
        utf8_emit_event1(id, "drop", data);
        return bool_value(current_len < high_water_mark);
    }
    UTF8_STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            if state
                .buffers
                .last()
                .is_none_or(|last| last.len().saturating_add(bytes.len()) > max_write)
            {
                state.buffers.push(bytes);
            } else if let Some(last) = state.buffers.last_mut() {
                last.extend_from_slice(&bytes);
            }
            state.len = new_len;
            update_utf8_props(state);
        }
    });
    utf8_maybe_drain_after_write(id);
    let len_after = UTF8_STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .map(|state| state.len)
            .unwrap_or(0)
    });
    bool_value(len_after < high_water_mark)
}

pub(crate) extern "C" fn utf8_stream_write_impl(closure: *const ClosureHeader, data: f64) -> f64 {
    utf8_stream_write_by_id(stream_id_of(closure), data)
}

pub(crate) fn utf8_stream_write_value(stream_value: f64, data: f64) -> Option<f64> {
    utf8_stream_id_from_value(stream_value).map(|id| utf8_stream_write_by_id(id, data))
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_write(stream_value: f64, data: f64) -> f64 {
    utf8_stream_write_value(stream_value, data).unwrap_or_else(undefined_value)
}

fn utf8_stream_flush_by_id(id: usize, callback: f64) -> f64 {
    if !is_undefined_value(callback)
        && !is_undefined_or_null(callback)
        && !is_callable_value(callback)
    {
        let message = format!(
            "The \"cb\" argument must be of type function. Received {}",
            crate::fs::validate::describe_received(callback)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    let (destroyed, min_length, opening_or_writing) = UTF8_STREAM_REGISTRY.with(|registry| {
        registry.borrow().get(&id).map_or((true, 0, false), |state| {
            (
                state.destroyed,
                state.min_length,
                state.opening || state.writing,
            )
        })
    });
    if destroyed {
        let err = crate::fs::validate::build_type_error_with_code_value(
            "Invalid state: Utf8Stream is destroyed",
            "ERR_INVALID_STATE",
        );
        if is_callable_value(callback) {
            let cb = extract_closure_ptr(callback);
            if !cb.is_null() {
                crate::closure::js_closure_call1(cb, err);
            }
            return undefined_value();
        }
        crate::exception::js_throw(err);
    }
    if min_length == 0 {
        if is_callable_value(callback) {
            let cb = extract_closure_ptr(callback);
            if !cb.is_null() {
                crate::closure::js_closure_call0(cb);
            }
        }
        return undefined_value();
    }
    if opening_or_writing {
        if is_callable_value(callback) {
            utf8_add_listener(id, "drain", callback, true);
        }
        return undefined_value();
    }
    let result = utf8_drain_buffers(id);
    if let Err(err) = result {
        utf8_emit_event1(id, "error", err);
        if is_callable_value(callback) {
            let cb = extract_closure_ptr(callback);
            if !cb.is_null() {
                crate::closure::js_closure_call1(cb, err);
            }
        }
    } else if is_callable_value(callback) {
        let cb = extract_closure_ptr(callback);
        if !cb.is_null() {
            crate::closure::js_closure_call0(cb);
        }
    }
    undefined_value()
}

pub(crate) extern "C" fn utf8_stream_flush_impl(
    closure: *const ClosureHeader,
    callback: f64,
) -> f64 {
    utf8_stream_flush_by_id(stream_id_of(closure), callback)
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_flush(stream_value: f64, callback: f64) -> f64 {
    match utf8_stream_id_from_value(stream_value) {
        Some(id) => utf8_stream_flush_by_id(id, callback),
        None => undefined_value(),
    }
}

fn utf8_stream_flush_sync_by_id(id: usize) -> f64 {
    let destroyed_or_bad_fd = UTF8_STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let Some(state) = registry.get(&id) else {
            return Some("Utf8Stream is destroyed");
        };
        if state.destroyed {
            Some("Utf8Stream is destroyed")
        } else if state.fd < 0 {
            Some("Invalid file descriptor")
        } else {
            None
        }
    });
    if let Some(message) = destroyed_or_bad_fd {
        crate::fs::validate::throw_error_with_code(
            &format!("Invalid state: {}", message),
            "ERR_INVALID_STATE",
        );
    }
    if let Err(err) = utf8_drain_buffers(id) {
        crate::exception::js_throw(err);
    }
    utf8_fsync(id);
    undefined_value()
}

pub(crate) extern "C" fn utf8_stream_flush_sync_impl(closure: *const ClosureHeader) -> f64 {
    utf8_stream_flush_sync_by_id(stream_id_of(closure))
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_flush_sync(stream_value: f64) -> f64 {
    match utf8_stream_id_from_value(stream_value) {
        Some(id) => utf8_stream_flush_sync_by_id(id),
        None => undefined_value(),
    }
}

fn utf8_emit_close_events(id: usize, emit_finish: bool) {
    if emit_finish {
        utf8_emit_event0(id, "finish");
    }
    utf8_emit_event0(id, "close");
}

extern "C" fn utf8_close_events_impl(closure: *const ClosureHeader) -> f64 {
    let id = stream_id_of(closure);
    let emit_finish = js_closure_get_capture_ptr(closure, 1) != 0;
    utf8_emit_close_events(id, emit_finish);
    undefined_value()
}

fn utf8_schedule_close_events(id: usize, emit_finish: bool) {
    let closure = js_closure_alloc(utf8_close_events_impl as *const u8, 2);
    js_closure_set_capture_ptr(closure, 0, id as i64);
    js_closure_set_capture_ptr(closure, 1, if emit_finish { 1 } else { 0 });
    crate::builtins::js_queue_microtask(closure as i64);
}

fn utf8_finish_and_close(id: usize, emit_finish: bool) {
    let (should_emit, sync) = UTF8_STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(state) = registry.get_mut(&id) else {
            return (false, true);
        };
        if state.destroyed {
            return (false, state.sync);
        }
        let sync = state.sync;
        state.destroyed = true;
        state.ending = emit_finish || state.ending;
        state.buffers.clear();
        state.len = 0;
        state.writing = false;
        update_utf8_props(state);
        (true, sync)
    });
    if should_emit {
        utf8_close_fd(id);
        if sync {
            utf8_emit_close_events(id, emit_finish);
        } else {
            utf8_schedule_close_events(id, emit_finish);
        }
    }
}

fn utf8_stream_end_by_id(id: usize) -> f64 {
    let (destroyed, opening) = UTF8_STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .map(|state| (state.destroyed, state.opening))
            .unwrap_or((true, false))
    });
    if destroyed {
        crate::fs::validate::throw_error_with_code(
            "Invalid state: Utf8Stream is destroyed",
            "ERR_INVALID_STATE",
        );
    }
    UTF8_STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.ending = true;
            update_utf8_props(state);
        }
    });
    if opening {
        return undefined_value();
    }
    if let Err(err) = utf8_drain_buffers(id) {
        utf8_emit_event1(id, "error", err);
    }
    utf8_finish_and_close(id, true);
    undefined_value()
}

pub(crate) extern "C" fn utf8_stream_end_impl(closure: *const ClosureHeader) -> f64 {
    utf8_stream_end_by_id(stream_id_of(closure))
}

pub(crate) fn utf8_stream_end_value(stream_value: f64, chunk: f64) -> Option<f64> {
    let id = utf8_stream_id_from_value(stream_value)?;
    if !is_undefined_value(chunk) && !is_undefined_or_null(chunk) {
        let _ = utf8_stream_write_by_id(id, chunk);
    }
    Some(utf8_stream_end_by_id(id))
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_end(stream_value: f64, chunk: f64) -> f64 {
    utf8_stream_end_value(stream_value, chunk).unwrap_or_else(undefined_value)
}

fn utf8_stream_destroy_by_id(id: usize) -> f64 {
    utf8_finish_and_close(id, false);
    undefined_value()
}

pub(crate) extern "C" fn utf8_stream_destroy_impl(closure: *const ClosureHeader) -> f64 {
    utf8_stream_destroy_by_id(stream_id_of(closure))
}

pub(crate) fn utf8_stream_destroy_value(stream_value: f64) -> Option<f64> {
    utf8_stream_id_from_value(stream_value).map(|id| {
        let _ = utf8_stream_destroy_by_id(id);
        undefined_value()
    })
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_destroy(stream_value: f64) -> f64 {
    utf8_stream_destroy_value(stream_value).unwrap_or_else(undefined_value)
}

fn utf8_stream_reopen_by_id(id: usize, file_value: f64) -> f64 {
    let (destroyed, existing_file, append, mkdir, sync, custom_fs) =
        UTF8_STREAM_REGISTRY.with(|registry| {
            let registry = registry.borrow();
            let Some(state) = registry.get(&id) else {
                return (true, None, true, false, true, undefined_value());
            };
            (
                state.destroyed,
                state.file.clone(),
                state.append,
                state.mkdir,
                state.sync,
                state.custom_fs,
            )
        });
    if destroyed {
        crate::fs::validate::throw_error_with_code(
            "Invalid state: Utf8Stream is destroyed",
            "ERR_INVALID_STATE",
        );
    }
    let Some(mut file) = existing_file else {
        crate::fs::validate::throw_error_with_code(
            "Unable to reopen a file descriptor, you must pass a file to SonicBoom",
            "ERR_OPERATION_FAILED",
        );
    };
    let new_path_value = if is_undefined_value(file_value) || is_undefined_or_null(file_value) {
        string_value_str(&file)
    } else {
        if !crate::fs::validate::is_path_like(file_value) {
            validate::throw_invalid_path_arg("file", file_value);
        }
        file = unsafe { decode_path_value(file_value).unwrap_or_default() };
        file_value
    };
    let old_fd = UTF8_STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .map(|state| state.fd)
            .unwrap_or(-1)
    });
    if !sync {
        UTF8_STREAM_REGISTRY.with(|registry| {
            if let Some(state) = registry.borrow_mut().get_mut(&id) {
                state.file = Some(file.clone());
                state.pending_file = Some(file);
                state.opening = true;
                state.writing = true;
                state.reopen_old_fd = Some(old_fd);
                state.closed = false;
                update_utf8_props(state);
            }
        });
        utf8_start_async_open(id);
        return undefined_value();
    }
    let new_fd = utf8_open_path(new_path_value, &file, append, mkdir, custom_fs);
    UTF8_STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.fd = new_fd;
            state.file = Some(file);
            state.closed = false;
            update_utf8_props(state);
        }
    });
    if old_fd >= 0 && old_fd != new_fd && fd_is_registered(old_fd) {
        let _ = js_fs_close_sync(old_fd as f64);
    }
    utf8_emit_event0(id, "ready");
    undefined_value()
}

pub(crate) extern "C" fn utf8_stream_reopen_impl(
    closure: *const ClosureHeader,
    file_value: f64,
) -> f64 {
    utf8_stream_reopen_by_id(stream_id_of(closure), file_value)
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_reopen(stream_value: f64, file_value: f64) -> f64 {
    match utf8_stream_id_from_value(stream_value) {
        Some(id) => utf8_stream_reopen_by_id(id, file_value),
        None => undefined_value(),
    }
}

pub(crate) extern "C" fn utf8_stream_on_impl(
    closure: *const ClosureHeader,
    event: f64,
    cb: f64,
) -> f64 {
    let id = stream_id_of(closure);
    utf8_add_listener(id, &event_name(event), cb, false);
    current_receiver_value()
}

pub(crate) extern "C" fn utf8_stream_once_impl(
    closure: *const ClosureHeader,
    event: f64,
    cb: f64,
) -> f64 {
    let id = stream_id_of(closure);
    utf8_add_listener(id, &event_name(event), cb, true);
    current_receiver_value()
}

pub(crate) extern "C" fn utf8_stream_off_impl(
    closure: *const ClosureHeader,
    event: f64,
    cb: f64,
) -> f64 {
    utf8_remove_listener(stream_id_of(closure), &event_name(event), cb);
    current_receiver_value()
}

pub(crate) extern "C" fn utf8_stream_remove_all_impl(
    closure: *const ClosureHeader,
    event: f64,
) -> f64 {
    let id = stream_id_of(closure);
    UTF8_STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            if is_undefined_value(event) {
                state.listeners.clear();
            } else {
                state.listeners.remove(&event_name(event));
            }
        }
    });
    current_receiver_value()
}

pub(crate) extern "C" fn utf8_stream_listener_count_impl(
    closure: *const ClosureHeader,
    event: f64,
) -> f64 {
    let id = stream_id_of(closure);
    let name = event_name(event);
    UTF8_STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .and_then(|state| state.listeners.get(&name))
            .map(|listeners| listeners.len() as f64)
            .unwrap_or(0.0)
    })
}

pub(crate) extern "C" fn utf8_stream_emit_impl(
    closure: *const ClosureHeader,
    event: f64,
    arg: f64,
) -> f64 {
    let id = stream_id_of(closure);
    let name = event_name(event);
    if is_undefined_value(arg) {
        utf8_emit_event0(id, &name);
    } else {
        utf8_emit_event1(id, &name, arg);
    }
    bool_value(true)
}

pub(crate) fn utf8_stream_on_value(
    stream_value: f64,
    event: f64,
    cb: f64,
    once: bool,
) -> Option<f64> {
    let id = utf8_stream_id_from_value(stream_value)?;
    utf8_add_listener(id, &event_name(event), cb, once);
    Some(stream_value)
}

pub(crate) fn utf8_stream_off_value(stream_value: f64, event: f64, cb: f64) -> Option<f64> {
    let id = utf8_stream_id_from_value(stream_value)?;
    utf8_remove_listener(id, &event_name(event), cb);
    Some(stream_value)
}

pub(crate) fn utf8_stream_remove_all_value(stream_value: f64, event: f64) -> Option<f64> {
    let id = utf8_stream_id_from_value(stream_value)?;
    UTF8_STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            if is_undefined_value(event) {
                state.listeners.clear();
            } else {
                state.listeners.remove(&event_name(event));
            }
        }
    });
    Some(stream_value)
}

pub(crate) fn utf8_stream_listener_count_value(stream_value: f64, event: f64) -> Option<f64> {
    let id = utf8_stream_id_from_value(stream_value)?;
    let name = event_name(event);
    Some(UTF8_STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .and_then(|state| state.listeners.get(&name))
            .map(|listeners| listeners.len() as f64)
            .unwrap_or(0.0)
    }))
}

pub(crate) fn utf8_stream_emit_value(stream_value: f64, event: f64, arg: f64) -> Option<f64> {
    let id = utf8_stream_id_from_value(stream_value)?;
    let name = event_name(event);
    if is_undefined_value(arg) {
        utf8_emit_event0(id, &name);
    } else {
        utf8_emit_event1(id, &name, arg);
    }
    Some(bool_value(true))
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_on(stream_value: f64, event: f64, cb: f64) -> f64 {
    utf8_stream_on_value(stream_value, event, cb, false).unwrap_or(stream_value)
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_once(stream_value: f64, event: f64, cb: f64) -> f64 {
    utf8_stream_on_value(stream_value, event, cb, true).unwrap_or(stream_value)
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_off(stream_value: f64, event: f64, cb: f64) -> f64 {
    utf8_stream_off_value(stream_value, event, cb).unwrap_or(stream_value)
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_remove_all(stream_value: f64, event: f64) -> f64 {
    utf8_stream_remove_all_value(stream_value, event).unwrap_or(stream_value)
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_listener_count(stream_value: f64, event: f64) -> f64 {
    utf8_stream_listener_count_value(stream_value, event).unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_emit(stream_value: f64, event: f64, arg: f64) -> f64 {
    utf8_stream_emit_value(stream_value, event, arg).unwrap_or_else(|| bool_value(false))
}

extern "C" fn utf8_periodic_flush_impl(closure: *const ClosureHeader) -> f64 {
    let id = stream_id_of(closure);
    let destroyed = UTF8_STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .map(|state| state.destroyed)
            .unwrap_or(true)
    });
    if !destroyed {
        let _ = utf8_drain_buffers(id);
    }
    undefined_value()
}

fn utf8_callback_has_error(err: f64) -> bool {
    !is_undefined_or_null(err) && crate::value::js_is_truthy(err) != 0
}

fn utf8_close_raw_fd(fd: i32, custom_fs: f64) {
    if fd < 0 {
        return;
    }
    if !is_undefined_value(custom_fs) && object_has_callable_field(custom_fs, b"closeSync") {
        let _ = utf8_call_custom_method(custom_fs, b"closeSync", &[fd as f64]);
    } else if fd_is_registered(fd) {
        let _ = js_fs_close_sync(fd as f64);
    }
}

fn utf8_async_open_error(id: usize, err: f64) {
    UTF8_STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.opening = false;
            state.writing = false;
            state.reopen_old_fd = None;
            update_utf8_props(state);
        }
    });
    utf8_emit_event1(id, "error", err);
}

fn utf8_async_open_finish(id: usize, err: f64, fd_value: f64) -> f64 {
    if utf8_callback_has_error(err) {
        utf8_async_open_error(id, err);
        return undefined_value();
    }
    let Some(fd) = js_i32_number_value(fd_value) else {
        let message = format!(
            "The \"fd\" argument must be of type number. Received {}",
            crate::fs::validate::describe_received(fd_value)
        );
        let err = crate::fs::validate::build_type_error_with_code_value(
            &message,
            "ERR_INVALID_ARG_TYPE",
        );
        utf8_async_open_error(id, err);
        return undefined_value();
    };
    let (destroyed, should_drain, should_end, old_fd, custom_fs) =
        UTF8_STREAM_REGISTRY.with(|registry| {
            let mut registry = registry.borrow_mut();
            let Some(state) = registry.get_mut(&id) else {
                return (true, false, false, None, undefined_value());
            };
            if state.destroyed {
                return (true, false, false, state.reopen_old_fd.take(), state.custom_fs);
            }
            state.fd = fd;
            state.file = state.pending_file.take();
            state.opening = false;
            state.writing = false;
            state.closed = false;
            let should_drain = state.len > state.min_length;
            let should_end = state.ending;
            let old_fd = state.reopen_old_fd.take();
            let custom_fs = state.custom_fs;
            update_utf8_props(state);
            (false, should_drain, should_end, old_fd, custom_fs)
        });
    if destroyed {
        utf8_close_raw_fd(fd, custom_fs);
        return undefined_value();
    }
    utf8_emit_event0(id, "ready");
    if let Some(old_fd) = old_fd {
        if old_fd != fd {
            utf8_close_raw_fd(old_fd, custom_fs);
        }
    }
    if should_drain || should_end {
        if let Err(err) = utf8_drain_buffers(id) {
            utf8_emit_event1(id, "error", err);
        }
    }
    if should_end {
        utf8_finish_and_close(id, true);
    } else if should_drain {
        utf8_emit_event0(id, "drain");
    }
    undefined_value()
}

extern "C" fn utf8_async_open_done_impl(
    closure: *const ClosureHeader,
    err: f64,
    fd_value: f64,
) -> f64 {
    utf8_async_open_finish(stream_id_of(closure), err, fd_value)
}

fn utf8_custom_open(id: usize) -> bool {
    let Some((file, append, mode_value, custom_fs)) = UTF8_STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let state = registry.get(&id)?;
        Some((
            state.pending_file.clone()?,
            state.append,
            state.mode_value,
            state.custom_fs,
        ))
    }) else {
        return true;
    };
    if is_undefined_value(custom_fs) || !object_has_callable_field(custom_fs, b"open") {
        return false;
    }
    let cb = js_closure_alloc(utf8_async_open_done_impl as *const u8, 1);
    js_closure_set_capture_ptr(cb, 0, id as i64);
    let cb_value = crate::value::js_nanbox_pointer(cb as i64);
    let flags_value = string_value_str(if append { "a" } else { "w" });
    let path_value = string_value_str(&file);
    let _ = utf8_call_custom_method(
        custom_fs,
        b"open",
        &[path_value, flags_value, mode_value, cb_value],
    );
    true
}

extern "C" fn utf8_async_mkdir_done_impl(closure: *const ClosureHeader, err: f64) -> f64 {
    let id = stream_id_of(closure);
    if utf8_callback_has_error(err) {
        utf8_async_open_error(id, err);
        return undefined_value();
    }
    if !utf8_custom_open(id) {
        utf8_schedule_native_open(id);
    }
    undefined_value()
}

fn utf8_custom_mkdir_then_open(id: usize) -> bool {
    let Some((file, custom_fs)) = UTF8_STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let state = registry.get(&id)?;
        if !state.mkdir {
            return None;
        }
        Some((state.pending_file.clone()?, state.custom_fs))
    }) else {
        return false;
    };
    if is_undefined_value(custom_fs) || !object_has_callable_field(custom_fs, b"mkdir") {
        return false;
    }
    let Some(parent) = utf8_stream_parent_dir(&file) else {
        return false;
    };
    let Some(parent_str) = parent.to_str() else {
        return false;
    };
    let options_obj = crate::object::js_object_alloc(0, 1);
    let key = js_string_from_bytes(b"recursive".as_ptr(), 9);
    crate::object::js_object_set_field_by_name(options_obj, key, bool_value(true));
    let options_value = object_value(options_obj);
    let cb = js_closure_alloc(utf8_async_mkdir_done_impl as *const u8, 1);
    js_closure_set_capture_ptr(cb, 0, id as i64);
    let cb_value = crate::value::js_nanbox_pointer(cb as i64);
    let parent_value = string_value_str(parent_str);
    let _ = utf8_call_custom_method(custom_fs, b"mkdir", &[parent_value, options_value, cb_value]);
    true
}

fn utf8_needs_native_mkdir_before_custom_open(id: usize) -> bool {
    UTF8_STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let Some(state) = registry.get(&id) else {
            return false;
        };
        state.mkdir
            && !is_undefined_value(state.custom_fs)
            && object_has_callable_field(state.custom_fs, b"open")
            && !object_has_callable_field(state.custom_fs, b"mkdir")
    })
}

fn utf8_schedule_native_mkdir_then_custom_open(id: usize) {
    let closure = js_closure_alloc(utf8_async_native_mkdir_then_custom_open_impl as *const u8, 1);
    js_closure_set_capture_ptr(closure, 0, id as i64);
    crate::builtins::js_queue_microtask(closure as i64);
}

fn utf8_schedule_native_open(id: usize) {
    let closure = js_closure_alloc(utf8_async_open_impl as *const u8, 1);
    js_closure_set_capture_ptr(closure, 0, id as i64);
    crate::builtins::js_queue_microtask(closure as i64);
}

fn utf8_start_async_open(id: usize) {
    if utf8_custom_mkdir_then_open(id) {
        return;
    }
    if utf8_needs_native_mkdir_before_custom_open(id) {
        utf8_schedule_native_mkdir_then_custom_open(id);
        return;
    }
    if utf8_custom_open(id) {
        return;
    }
    utf8_schedule_native_open(id);
}

extern "C" fn utf8_async_native_mkdir_then_custom_open_impl(
    closure: *const ClosureHeader,
) -> f64 {
    let id = stream_id_of(closure);
    let Some(file) = UTF8_STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .and_then(|state| state.pending_file.clone())
    }) else {
        return undefined_value();
    };
    if let Err(err) = utf8_native_mkdir_result(&file) {
        utf8_async_open_error(id, err);
        return undefined_value();
    }
    if !utf8_custom_open(id) {
        utf8_schedule_native_open(id);
    }
    undefined_value()
}

extern "C" fn utf8_async_open_impl(closure: *const ClosureHeader) -> f64 {
    let id = stream_id_of(closure);
    let Some(file) = UTF8_STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .and_then(|state| state.pending_file.clone())
    }) else {
        return undefined_value();
    };
    let (append, mkdir) = UTF8_STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let Some(state) = registry.get(&id) else {
            return (true, false);
        };
        (state.append, state.mkdir)
    });
    let path_value = string_value_str(&file);
    match utf8_native_async_open_path_result(path_value, &file, append, mkdir) {
        Ok(fd) => utf8_async_open_finish(id, null_value(), fd as f64),
        Err(err) => utf8_async_open_finish(id, err, undefined_value()),
    }
}

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
