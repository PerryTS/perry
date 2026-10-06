//! Chunk and byte collection for readable streams: flattening a stream's
//! hidden chunk storage, arrays, buffers and strings into bytes or chunk
//! values. A child module of node_stream_readwrite.rs, so `use super::*`
//! reaches the parent's private items.

use super::*;

fn append_string_bytes(value: f64, out: &mut Vec<u8>) {
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const crate::StringHeader;
    append_string_ptr_bytes(ptr, out);
}

fn append_string_ptr_bytes(ptr: *const crate::StringHeader, out: &mut Vec<u8>) {
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return;
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        out.extend_from_slice(std::slice::from_raw_parts(data, len));
    }
}

pub(in crate::node_stream) fn append_buffer_bytes(raw: usize, out: &mut Vec<u8>) {
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

fn append_array_chunks(raw: usize, out: &mut Vec<u8>, depth: u8) {
    if raw < 0x10000 {
        return;
    }
    let arr = raw as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    for i in 0..len {
        let chunk = crate::array::js_array_get_f64(arr, i);
        append_chunk_bytes(chunk, out, depth + 1);
    }
}

pub(in crate::node_stream) fn append_chunk_bytes(value: f64, out: &mut Vec<u8>, depth: u8) {
    if depth > 8 {
        return;
    }
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_any_string() {
        append_string_bytes(value, out);
        return;
    }
    if jsval.is_int32() {
        out.extend_from_slice(jsval.as_int32().to_string().as_bytes());
        return;
    }
    if jsval.is_number() && value.is_finite() {
        let text = if value.fract() == 0.0 {
            (value as i64).to_string()
        } else {
            value.to_string()
        };
        out.extend_from_slice(text.as_bytes());
        return;
    }

    let raw = raw_ptr_from_value(value);
    if raw < 0x10000 {
        return;
    }
    if crate::buffer::is_registered_buffer(raw) {
        append_buffer_bytes(raw, out);
        return;
    }

    unsafe {
        match gc_type_for_ptr(raw) {
            Some(crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_LAZY_ARRAY) => {
                append_array_chunks(raw, out, depth);
            }
            Some(crate::gc::GC_TYPE_OBJECT) => {
                if let Some(chunks) = readable_hidden_chunks(value) {
                    append_chunk_bytes(chunks, out, depth + 1);
                }
            }
            Some(crate::gc::GC_TYPE_STRING) => {
                append_string_ptr_bytes(raw as *const crate::StringHeader, out);
            }
            _ => {}
        }
    }
}

pub(in crate::node_stream) fn push_chunk_values(value: f64, out: &mut Vec<f64>, depth: u8) {
    if depth > 8 {
        return;
    }
    if let Some(chunks) = readable_hidden_chunks(value) {
        push_chunk_values(chunks, out, depth + 1);
        return;
    }
    if is_array_like_value(value) {
        let raw = raw_ptr_from_value(value);
        if raw < 0x10000 {
            return;
        }
        let arr = raw as *const crate::array::ArrayHeader;
        let len = crate::array::js_array_length(arr);
        for i in 0..len {
            out.push(crate::array::js_array_get_f64(arr, i));
        }
        return;
    }
    if is_single_chunk_value(value) {
        out.push(value);
    }
}

/// Drain the chunk storage Perry attaches in `Readable.from(iterable)`.
pub fn js_node_stream_collect_bytes(stream: f64) -> Vec<u8> {
    js_node_stream_collect_bytes_result(stream).unwrap_or_default()
}

pub fn js_node_stream_collect_chunks_result(stream: f64) -> Option<Result<f64, f64>> {
    invoke_read_once(stream);
    if let Some(err) = readable_hidden_error(stream) {
        return Some(Err(err));
    }
    if let Some(chunks) = readable_hidden_chunks(stream) {
        return Some(Ok(chunks));
    }
    if is_array_like_value(stream) {
        return Some(Ok(stream));
    }
    if is_single_chunk_value(stream) {
        let mut arr = crate::array::js_array_alloc(1);
        arr = crate::array::js_array_push_f64(arr, stream);
        return Some(Ok(box_pointer(arr as *const u8)));
    }
    if get_hidden_value(stream, hidden_read_key()).is_some() {
        let arr = crate::array::js_array_alloc(0);
        return Some(Ok(box_pointer(arr as *const u8)));
    }
    None
}

pub fn js_node_stream_collect_bytes_result(stream: f64) -> Result<Vec<u8>, f64> {
    invoke_read_once(stream);
    if let Some(err) = readable_hidden_error(stream) {
        return Err(err);
    }
    let mut out = Vec::new();
    append_chunk_bytes(stream, &mut out, 0);
    if let Some(err) = readable_hidden_error(stream) {
        return Err(err);
    }
    Ok(out)
}

/// The chunks a pipeline source holds, copied into one fresh GC array rooted
/// in `scope`, or `None` when it holds none.
///
/// `_read` and the hidden-property reads (generic lookups, so a getter may
/// run) can collect, so the stream is reread from its handle after each and
/// the chunks go straight into a GC array the collector rewrites.
pub(crate) fn js_node_stream_readable_chunks_result<'s>(
    scope: &'s crate::gc::RuntimeHandleScope,
    stream: &crate::gc::RuntimeHandle<'_>,
) -> Result<Option<crate::gc::RuntimeHandle<'s>>, f64> {
    invoke_read_once(stream.get_nanbox_f64());
    if let Some(err) = readable_hidden_error(stream.get_nanbox_f64()) {
        return Err(err);
    }
    let Some(chunks) = readable_hidden_chunks(stream.get_nanbox_f64()) else {
        return Ok(None);
    };
    let chunks = scope.root_nanbox_f64(chunks);
    let out = scope.root_raw_mut_ptr(crate::array::js_array_alloc(0));
    append_chunk_values(&chunks, &out, 0);
    if let Some(err) = readable_hidden_error(stream.get_nanbox_f64()) {
        return Err(err);
    }
    Ok(Some(out))
}

/// [`push_chunk_values`] into a rooted GC array: `value` is reread from its
/// handle after every step that can collect.
fn append_chunk_values(
    value: &crate::gc::RuntimeHandle<'_>,
    out: &crate::gc::RuntimeHandle<'_>,
    depth: u8,
) {
    if depth > 8 {
        return;
    }
    // `js_array_push_f64` roots its receiver and the value before it can grow.
    let push = |chunk: f64| {
        out.set_raw_mut_ptr(out.with_mut_ptr(|arr: *mut crate::array::ArrayHeader| {
            crate::array::js_array_push_f64(arr, chunk)
        }));
    };
    if let Some(chunks) = readable_hidden_chunks(value.get_nanbox_f64()) {
        let scope = crate::gc::RuntimeHandleScope::new();
        append_chunk_values(&scope.root_nanbox_f64(chunks), out, depth + 1);
        return;
    }
    // `is_array_like_value` has already rejected the handle band.
    if is_array_like_value(value.get_nanbox_f64()) {
        let arr = || raw_ptr_from_value(value.get_nanbox_f64()) as *const crate::array::ArrayHeader;
        let mut i = 0;
        while i < crate::array::js_array_length(arr()) {
            push(crate::array::js_array_get_f64(arr(), i));
            i += 1;
        }
        return;
    }
    if is_single_chunk_value(value.get_nanbox_f64()) {
        push(value.get_nanbox_f64());
    }
}
