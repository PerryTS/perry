//! `node:stream/consumers` (`text`, `json`, `buffer`, `arrayBuffer`, `bytes`,
//! `blob`) — concrete chunk-collection behavior plus the buffer/byte helpers
//! the blob module also reuses.
//!
//! Extracted from `mod.rs` so the parent module stays under the file-size
//! gate. Pure code movement — no logic changes.
//!
//! NOTE: this module keeps its own copies of `raw_ptr_from_value` /
//! `gc_type_for_ptr` / `object_ptr_from_value` (here returning `*const
//! ObjectHeader`). The stream/promises module has near-identical helpers with
//! slightly different signatures; they intentionally live in separate module
//! scopes so the names don't collide at module root.

use std::sync::atomic::{AtomicPtr, Ordering};

use super::blob::blob_value_from_bytes;
use super::fs_promises::{promise_rejected, promise_value};
use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_get_capture_ptr,
    js_closure_set_capture_f64, js_closure_set_capture_ptr, ClosureHeader,
};
use crate::object::{js_object_get_field_by_name_f64, ObjectHeader};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;

pub(crate) fn buffer_from_bytes(
    bytes: &[u8],
    mark_array_buffer: bool,
    mark_uint8_array: bool,
) -> *mut crate::buffer::BufferHeader {
    let buf = crate::buffer::buffer_alloc(bytes.len() as u32);
    unsafe {
        (*buf).length = bytes.len() as u32;
        if !bytes.is_empty() {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                crate::buffer::buffer_data_mut(buf),
                bytes.len(),
            );
        }
    }
    if mark_array_buffer {
        crate::buffer::mark_as_array_buffer(buf as usize);
    }
    if mark_uint8_array {
        crate::buffer::mark_as_uint8array(buf as usize);
    }
    buf
}

pub(crate) fn bytes_to_buffer_value(bytes: &[u8]) -> f64 {
    let buf = buffer_from_bytes(bytes, false, false);
    f64::from_bits(JSValue::pointer(buf as *const u8).bits())
}

pub(crate) fn bytes_to_array_buffer_value(bytes: &[u8]) -> f64 {
    let buf = buffer_from_bytes(bytes, true, false);
    f64::from_bits(JSValue::pointer(buf as *const u8).bits())
}

pub(crate) fn bytes_to_uint8_array_value(bytes: &[u8]) -> f64 {
    let buf = buffer_from_bytes(bytes, false, true);
    f64::from_bits(JSValue::pointer(buf as *const u8).bits())
}

pub(crate) fn bytes_to_text_value(bytes: &[u8]) -> f64 {
    let ptr = crate::buffer::buf_bytes_to_utf8_string(bytes);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

#[derive(Clone, Copy)]
enum ConsumerKind {
    Text = 0,
    Json = 1,
    Buffer = 2,
    ArrayBuffer = 3,
    Bytes = 4,
    Blob = 5,
}

impl ConsumerKind {
    fn from_i64(value: i64) -> Self {
        match value {
            1 => Self::Json,
            2 => Self::Buffer,
            3 => Self::ArrayBuffer,
            4 => Self::Bytes,
            5 => Self::Blob,
            _ => Self::Text,
        }
    }

    fn chunk_mode(self) -> ChunkMode {
        match self {
            Self::Text | Self::Json => ChunkMode::Text,
            Self::Buffer | Self::ArrayBuffer | Self::Bytes | Self::Blob => ChunkMode::Binary,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChunkMode {
    Binary,
    Text,
}

#[derive(Clone, Copy)]
enum CollectMethod {
    Next = 0,
    Read = 1,
}

type StreamGetReaderFn = unsafe extern "C" fn(f64) -> f64;
type StreamReaderReadFn = unsafe extern "C" fn(f64) -> *mut crate::Promise;

static STREAM_CONSUMER_GET_READER: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static STREAM_CONSUMER_READER_READ: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

#[no_mangle]
pub extern "C" fn js_register_stream_consumer_callbacks(
    get_reader: StreamGetReaderFn,
    reader_read: StreamReaderReadFn,
) {
    STREAM_CONSUMER_GET_READER.store(get_reader as *mut (), Ordering::Release);
    STREAM_CONSUMER_READER_READ.store(reader_read as *mut (), Ordering::Release);
}

impl CollectMethod {
    fn from_i64(value: i64) -> Self {
        if value == 1 {
            Self::Read
        } else {
            Self::Next
        }
    }

    fn name(self) -> &'static [u8] {
        match self {
            Self::Next => b"next",
            Self::Read => b"read",
        }
    }
}

fn boxed_pointer(ptr: *const u8) -> f64 {
    f64::from_bits(JSValue::pointer(ptr).bits())
}

fn is_integral_handle_value(value: f64) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    (jsval.is_int32() && jsval.as_int32() > 0)
        || (jsval.is_number() && value.is_finite() && value > 0.0 && value.fract() == 0.0)
}

fn is_undefined_value(value: f64) -> bool {
    value.to_bits() == crate::value::TAG_UNDEFINED
        || JSValue::from_bits(value.to_bits()).is_undefined()
}

fn raw_ptr_from_value(value: f64) -> usize {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);
    if jsval.is_pointer() || jsval.is_string() || jsval.is_bigint() {
        return (bits & crate::value::POINTER_MASK) as usize;
    }
    if bits != 0 && bits < 0x0001_0000_0000_0000 {
        return bits as usize;
    }
    0
}

unsafe fn gc_type_for_ptr(raw: usize) -> Option<u8> {
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

fn object_ptr_from_value(value: f64) -> Option<*const ObjectHeader> {
    let raw = raw_ptr_from_value(value);
    if raw < 0x10000 || crate::buffer::is_registered_buffer(raw) {
        return None;
    }
    unsafe {
        if gc_type_for_ptr(raw) != Some(crate::gc::GC_TYPE_OBJECT) {
            return None;
        }
    }
    Some(raw as *const ObjectHeader)
}

fn named_key(bytes: &[u8]) -> *const StringHeader {
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn get_named_value(value: f64, name: &[u8]) -> f64 {
    let Some(obj) = object_ptr_from_value(value) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    let key = named_key(name);
    js_object_get_field_by_name_f64(obj, key)
}

fn is_callable_value(value: f64) -> bool {
    let raw = raw_ptr_from_value(value);
    if raw < 0x10000 || crate::buffer::is_registered_buffer(raw) {
        return false;
    }
    unsafe {
        gc_type_for_ptr(raw) == Some(crate::gc::GC_TYPE_CLOSURE)
            && crate::closure::is_closure_ptr(raw)
    }
}

fn has_named_callable(value: f64, name: &[u8]) -> bool {
    is_callable_value(get_named_value(value, name))
}

fn invalid_chunk_error(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());
    let (kind, detail) = if jsval.is_int32() {
        ("number", format!(" ({})", jsval.as_int32()))
    } else if jsval.is_number() {
        ("number", format!(" ({})", value))
    } else if jsval.is_bool() {
        ("boolean", String::new())
    } else if jsval.is_undefined() {
        ("undefined", String::new())
    } else if jsval.is_null() {
        ("null", String::new())
    } else if is_callable_value(value) {
        ("function", String::new())
    } else {
        ("object", String::new())
    };
    let msg = format!(
        "The \"chunk\" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received type {}{}",
        kind, detail
    );
    let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    crate::node_submodules::register_error_code_pub(msg_ptr, "ERR_INVALID_ARG_TYPE");
    let err = crate::error::js_typeerror_new(msg_ptr);
    boxed_pointer(err as *const u8)
}

fn invalid_stream_error() -> f64 {
    let msg = b"stream is not async iterable";
    let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(msg_ptr);
    boxed_pointer(err as *const u8)
}

fn append_string_value_bytes(value: f64, out: &mut Vec<u8>) {
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    append_string_ptr_bytes(ptr, out);
}

fn append_string_ptr_bytes(ptr: *const StringHeader, out: &mut Vec<u8>) {
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return;
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        out.extend_from_slice(std::slice::from_raw_parts(data, len));
    }
}

fn append_buffer_value_bytes(raw: usize, out: &mut Vec<u8>) {
    if raw < 0x10000 || !crate::buffer::is_registered_buffer(raw) {
        return;
    }
    unsafe {
        let buf = raw as *const crate::buffer::BufferHeader;
        let len = (*buf).length as usize;
        let data = crate::buffer::buffer_data(buf);
        out.extend_from_slice(std::slice::from_raw_parts(data, len));
    }
}

fn append_number_chunk(value: f64, jsval: JSValue, out: &mut Vec<u8>) {
    let text = if jsval.is_int32() {
        jsval.as_int32().to_string()
    } else if value.is_finite() && value.fract() == 0.0 {
        (value as i64).to_string()
    } else {
        value.to_string()
    };
    out.extend_from_slice(text.as_bytes());
}

fn append_array_chunk_bytes(
    raw: usize,
    out: &mut Vec<u8>,
    mode: ChunkMode,
    depth: u8,
) -> Result<(), f64> {
    if raw < 0x10000 {
        return Ok(());
    }
    let arr = raw as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    for i in 0..len {
        let chunk = crate::array::js_array_get_f64(arr, i);
        append_chunk_bytes_for_consumer(chunk, out, mode, depth + 1)?;
    }
    Ok(())
}

fn append_chunk_bytes_for_consumer(
    value: f64,
    out: &mut Vec<u8>,
    mode: ChunkMode,
    depth: u8,
) -> Result<(), f64> {
    if depth > 16 {
        return Err(invalid_chunk_error(value));
    }
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_any_string() {
        append_string_value_bytes(value, out);
        return Ok(());
    }
    if jsval.is_int32() || (jsval.is_number() && value.is_finite()) {
        if mode == ChunkMode::Text {
            return Err(invalid_chunk_error(value));
        }
        append_number_chunk(value, jsval, out);
        return Ok(());
    }

    let raw = raw_ptr_from_value(value);
    if raw < 0x10000 {
        return Err(invalid_chunk_error(value));
    }
    if crate::buffer::is_registered_buffer(raw) {
        append_buffer_value_bytes(raw, out);
        return Ok(());
    }

    unsafe {
        match gc_type_for_ptr(raw) {
            Some(crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_LAZY_ARRAY) => {
                append_array_chunk_bytes(raw, out, mode, depth)
            }
            Some(crate::gc::GC_TYPE_OBJECT) => {
                if let Some(Ok(chunks)) =
                    crate::node_stream::js_node_stream_collect_chunks_result(value)
                {
                    append_chunk_bytes_for_consumer(chunks, out, mode, depth + 1)
                } else {
                    Err(invalid_chunk_error(value))
                }
            }
            Some(crate::gc::GC_TYPE_STRING) => {
                append_string_ptr_bytes(raw as *const StringHeader, out);
                Ok(())
            }
            _ => Err(invalid_chunk_error(value)),
        }
    }
}

fn chunks_to_bytes(chunks: f64, mode: ChunkMode) -> Result<Vec<u8>, f64> {
    let mut out = Vec::new();
    append_chunk_bytes_for_consumer(chunks, &mut out, mode, 0)?;
    Ok(out)
}

fn finish_consumer_from_chunks(kind: ConsumerKind, chunks: f64) -> Result<f64, f64> {
    let bytes = chunks_to_bytes(chunks, kind.chunk_mode())?;
    match kind {
        ConsumerKind::Text => Ok(bytes_to_text_value(&bytes)),
        ConsumerKind::Json => {
            let text = bytes_to_text_value(&bytes);
            let text_ptr = crate::value::js_get_string_pointer_unified(text) as *const StringHeader;
            unsafe { crate::json::js_json_parse_result(text_ptr).map(|v| f64::from_bits(v.bits())) }
        }
        ConsumerKind::Buffer => Ok(bytes_to_buffer_value(&bytes)),
        ConsumerKind::ArrayBuffer => Ok(bytes_to_array_buffer_value(&bytes)),
        ConsumerKind::Bytes => Ok(bytes_to_uint8_array_value(&bytes)),
        ConsumerKind::Blob => Ok(blob_value_from_bytes(&bytes)),
    }
}

fn promise_from_consumer_chunks(kind: ConsumerKind, chunks: Result<f64, f64>) -> f64 {
    match chunks.and_then(|chunks| finish_consumer_from_chunks(kind, chunks)) {
        Ok(value) => promise_value(value),
        Err(err) => promise_rejected(err),
    }
}

fn settle_consumer_from_chunks(promise: *mut crate::Promise, kind: ConsumerKind, chunks: f64) {
    if promise.is_null() {
        return;
    }
    match finish_consumer_from_chunks(kind, chunks) {
        Ok(value) => crate::promise::js_promise_resolve(promise, value),
        Err(err) => crate::promise::js_promise_reject(promise, err),
    }
}

fn promise_ptr_from_value(value: f64) -> Option<*mut crate::Promise> {
    if crate::promise::js_value_is_promise(value) == 0 {
        return None;
    }
    let raw = raw_ptr_from_value(value);
    if raw < 0x10000 {
        None
    } else {
        Some(raw as *mut crate::Promise)
    }
}

fn registered_readable_stream_reader(stream: f64) -> Option<f64> {
    if !is_integral_handle_value(stream) {
        return None;
    }
    let f = STREAM_CONSUMER_GET_READER.load(Ordering::Acquire);
    if f.is_null() {
        return None;
    }
    let reader = unsafe {
        let func: StreamGetReaderFn = std::mem::transmute(f);
        func(stream)
    };
    if is_undefined_value(reader) {
        None
    } else {
        Some(reader)
    }
}

fn registered_reader_read_promise(reader: f64) -> Option<*mut crate::Promise> {
    if !is_integral_handle_value(reader) {
        return None;
    }
    let f = STREAM_CONSUMER_READER_READ.load(Ordering::Acquire);
    if f.is_null() {
        return None;
    }
    Some(unsafe {
        let func: StreamReaderReadFn = std::mem::transmute(f);
        func(reader)
    })
}

fn call_collector_method(
    receiver: f64,
    method: CollectMethod,
    step: *const ClosureHeader,
    reject: *const ClosureHeader,
) {
    if let CollectMethod::Read = method {
        if let Some(promise) = registered_reader_read_promise(receiver) {
            crate::promise::js_promise_then(promise, step, reject);
            return;
        }
    }

    let name = method.name();
    let result = unsafe {
        crate::object::js_native_call_method(
            receiver,
            name.as_ptr() as *const i8,
            name.len(),
            std::ptr::null(),
            0,
        )
    };
    if let Some(promise) = promise_ptr_from_value(result) {
        crate::promise::js_promise_then(promise, step, reject);
    } else {
        consumer_collect_step(step, result);
    }
}

fn collect_by_method_promise(kind: ConsumerKind, receiver: f64, method: CollectMethod) -> f64 {
    let result_promise = crate::promise::js_promise_new();
    let result_arr = crate::array::js_array_alloc(0);
    let step = js_closure_alloc(consumer_collect_step as *const u8, 6);
    let reject = js_closure_alloc(consumer_collect_rejected as *const u8, 1);
    js_closure_set_capture_ptr(step, 0, result_promise as i64);
    js_closure_set_capture_ptr(step, 1, result_arr as i64);
    js_closure_set_capture_f64(step, 2, receiver);
    js_closure_set_capture_ptr(step, 3, reject as i64);
    js_closure_set_capture_ptr(step, 4, method as i64);
    js_closure_set_capture_ptr(step, 5, kind as i64);
    js_closure_set_capture_ptr(reject, 0, result_promise as i64);
    call_collector_method(receiver, method, step, reject);
    boxed_pointer(result_promise as *const u8)
}

fn call_symbol_async_iterator(stream: f64) -> Option<f64> {
    let sym = crate::symbol::well_known_symbol("asyncIterator");
    if sym.is_null() {
        return None;
    }
    let sym_f64 = boxed_pointer(sym as *const u8);
    let method = unsafe { crate::symbol::js_object_get_symbol_property(stream, sym_f64) };
    if !is_callable_value(method) {
        return None;
    }
    let prev_this = crate::object::js_implicit_this_set(stream);
    let iterator = unsafe { crate::closure::js_native_call_value(method, std::ptr::null(), 0) };
    crate::object::js_implicit_this_set(prev_this);
    if iterator.to_bits() == crate::value::TAG_UNDEFINED {
        None
    } else {
        Some(iterator)
    }
}

fn async_consumer_promise(kind: ConsumerKind, stream: f64) -> Option<f64> {
    if let Some(iterator) = call_symbol_async_iterator(stream) {
        if has_named_callable(iterator, b"next") {
            return Some(collect_by_method_promise(
                kind,
                iterator,
                CollectMethod::Next,
            ));
        }
    }
    if has_named_callable(stream, b"next") {
        return Some(collect_by_method_promise(kind, stream, CollectMethod::Next));
    }
    if has_named_callable(stream, b"getReader") {
        let reader = unsafe {
            crate::object::js_native_call_method(
                stream,
                b"getReader".as_ptr() as *const i8,
                b"getReader".len(),
                std::ptr::null(),
                0,
            )
        };
        if reader.to_bits() == crate::value::TAG_UNDEFINED {
            return Some(promise_rejected(invalid_chunk_error(stream)));
        }
        return Some(collect_by_method_promise(kind, reader, CollectMethod::Read));
    }
    if let Some(reader) = registered_readable_stream_reader(stream) {
        return Some(collect_by_method_promise(kind, reader, CollectMethod::Read));
    }
    None
}

fn consume_stream(kind: ConsumerKind, stream: f64) -> f64 {
    if let Some(chunks) = crate::node_stream::js_node_stream_collect_chunks_result(stream) {
        return promise_from_consumer_chunks(kind, chunks);
    }
    if let Some(promise) = async_consumer_promise(kind, stream) {
        return promise;
    }
    promise_rejected(invalid_stream_error())
}

extern "C" fn consumer_collect_rejected(closure: *const ClosureHeader, reason: f64) -> f64 {
    let promise = js_closure_get_capture_ptr(closure, 0) as *mut crate::Promise;
    crate::promise::js_promise_reject(promise, reason);
    0.0
}

extern "C" fn consumer_collect_step(closure: *const ClosureHeader, iter_result: f64) -> f64 {
    let promise = js_closure_get_capture_ptr(closure, 0) as *mut crate::Promise;
    let mut result_arr = js_closure_get_capture_ptr(closure, 1) as *mut crate::array::ArrayHeader;
    let receiver = js_closure_get_capture_f64(closure, 2);
    let reject = js_closure_get_capture_ptr(closure, 3) as *const ClosureHeader;
    let method = CollectMethod::from_i64(js_closure_get_capture_ptr(closure, 4));
    let kind = ConsumerKind::from_i64(js_closure_get_capture_ptr(closure, 5));
    if promise.is_null() || result_arr.is_null() {
        return 0.0;
    }

    let Some(result_obj) = object_ptr_from_value(iter_result) else {
        let arr_value = boxed_pointer(result_arr as *const u8);
        settle_consumer_from_chunks(promise, kind, arr_value);
        return 0.0;
    };

    let done = js_object_get_field_by_name_f64(result_obj, named_key(b"done"));
    if crate::value::js_is_truthy(done) != 0 {
        let arr_value = boxed_pointer(result_arr as *const u8);
        settle_consumer_from_chunks(promise, kind, arr_value);
        return 0.0;
    }

    let value = js_object_get_field_by_name_f64(result_obj, named_key(b"value"));
    result_arr = crate::array::js_array_push_f64(result_arr, value);
    js_closure_set_capture_ptr(closure as *mut ClosureHeader, 1, result_arr as i64);
    call_collector_method(receiver, method, closure, reject);
    0.0
}

pub(crate) extern "C" fn thunk_consumers_text(_closure: *const ClosureHeader, stream: f64) -> f64 {
    consume_stream(ConsumerKind::Text, stream)
}

pub(crate) extern "C" fn thunk_consumers_json(_closure: *const ClosureHeader, stream: f64) -> f64 {
    consume_stream(ConsumerKind::Json, stream)
}

pub(crate) extern "C" fn thunk_consumers_buffer(
    _closure: *const ClosureHeader,
    stream: f64,
) -> f64 {
    consume_stream(ConsumerKind::Buffer, stream)
}

pub(crate) extern "C" fn thunk_consumers_arrayBuffer(
    _closure: *const ClosureHeader,
    stream: f64,
) -> f64 {
    consume_stream(ConsumerKind::ArrayBuffer, stream)
}

pub(crate) extern "C" fn thunk_consumers_bytes(_closure: *const ClosureHeader, stream: f64) -> f64 {
    consume_stream(ConsumerKind::Bytes, stream)
}

pub(crate) extern "C" fn thunk_consumers_blob(_closure: *const ClosureHeader, stream: f64) -> f64 {
    consume_stream(ConsumerKind::Blob, stream)
}
