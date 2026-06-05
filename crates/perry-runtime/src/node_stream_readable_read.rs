//! Readable `read()` consumption helpers, split from node_stream_readwrite.rs.
use super::*;

pub(super) fn read_stream_with_size_arg(stream: f64, size: f64) -> f64 {
    let size_value = JSValue::from_bits(size.to_bits());
    if size_value.is_undefined() || !size_value.is_number() {
        return read_stream_default_size(stream);
    }
    let size = size_value.as_number();
    if size.is_nan() {
        return read_stream_default_size(stream);
    }
    read_stream_exact_size(stream, size.trunc())
}

pub(super) fn read_stream_default_size(stream: f64) -> f64 {
    invoke_read_once(stream);
    read_stream_available_default(stream)
}

pub(super) fn read_stream_available_default(stream: f64) -> f64 {
    if get_hidden_value(stream, hidden_buffered_key()).unwrap_or(0.0) <= 0.0 {
        if stream_hidden_ended(stream) {
            cancel_readable_event(stream);
            refresh_readable_aborted_flag(stream);
        }
        return f64::from_bits(TAG_NULL);
    }
    if readable_object_mode(stream) {
        return read_stream_object_mode_chunk(stream);
    }

    // `read()` with no size argument drains the entire internal buffer and
    // returns it as a single value (Node semantics). The chunks are stored as
    // separate entries to preserve boundaries for sized `read(n)` calls, so we
    // gather them all here and concatenate before clearing the buffer.
    let mut values = Vec::new();
    if let Some(chunks) = readable_hidden_chunks(stream) {
        push_chunk_values(chunks, &mut values, 0);
    }
    if values.is_empty() {
        if stream_hidden_ended(stream) {
            cancel_readable_event(stream);
            refresh_readable_aborted_flag(stream);
        }
        return f64::from_bits(TAG_NULL);
    }

    clear_readable_buffer(stream);
    mark_disturbed(stream);
    clear_pending_readable_chunks(stream);
    if stream_hidden_ended(stream) {
        queue_readable_event(stream);
        schedule_readable_end(stream);
    }

    let encoded = readable_encoding_tag(stream).is_some();
    if encoded {
        let mut decoded = Vec::with_capacity(values.len());
        for value in values {
            if let Some(value) = super::decode_readable_chunk_for_encoding(stream, value) {
                decoded.push(value);
            }
        }
        values = decoded;
        if values.is_empty() {
            return f64::from_bits(TAG_NULL);
        }
        if values.len() == 1 {
            return values[0];
        }
        let result = crate::string::js_string_concat_chain(values.as_ptr(), values.len() as i32);
        return f64::from_bits(JSValue::string_ptr(result).bits());
    }

    let mut bytes = Vec::new();
    for value in &values {
        append_chunk_bytes(*value, &mut bytes, 0);
    }
    buffer_value_from_bytes(&bytes)
}

pub(super) fn read_stream_exact_size(stream: f64, size: f64) -> f64 {
    invoke_read_once(stream);
    if size <= 0.0 {
        return f64::from_bits(TAG_NULL);
    }
    let requested = size as usize;
    let available = get_hidden_value(stream, hidden_buffered_key())
        .unwrap_or(0.0)
        .max(0.0) as usize;
    if available == 0 {
        if stream_hidden_ended(stream) {
            cancel_readable_event(stream);
            refresh_readable_aborted_flag(stream);
        }
        return f64::from_bits(TAG_NULL);
    }
    if readable_encoding_tag(stream).is_some() {
        return read_stream_available_default(stream);
    }
    if requested > available && !stream_hidden_ended(stream) {
        return f64::from_bits(TAG_NULL);
    }
    if requested >= available {
        return read_stream_exact_bytes(stream, available);
    }

    read_stream_exact_bytes(stream, requested)
}

fn sync_pending_readable_chunks_to_buffer(stream: f64) {
    let mut pending = crate::array::js_array_alloc(0);
    if let Some(chunks) = readable_hidden_chunks(stream) {
        if is_array_like_value(chunks) {
            let arr = raw_ptr_from_value(chunks) as *const crate::array::ArrayHeader;
            let len = crate::array::js_array_length(arr);
            for i in 0..len {
                pending = crate::array::js_array_push_f64(
                    pending,
                    crate::array::js_array_get_f64(arr, i),
                );
            }
        } else if is_single_chunk_value(chunks) {
            pending = crate::array::js_array_push_f64(pending, chunks);
        }
    }
    set_hidden_value(
        stream,
        hidden_readable_pending_key(),
        box_pointer(pending as *const u8),
    );
}

fn set_readable_buffer_values(stream: f64, values: &[f64], byte_len: usize) {
    if values.is_empty() || byte_len == 0 {
        clear_readable_buffer(stream);
        sync_pending_readable_chunks_to_buffer(stream);
        return;
    }
    let mut arr = crate::array::js_array_alloc(values.len() as u32);
    for value in values {
        arr = crate::array::js_array_push_f64(arr, *value);
    }
    set_hidden_value(stream, hidden_chunks_key(), box_pointer(arr as *const u8));
    let remaining = byte_len as f64;
    set_hidden_value(stream, hidden_buffered_key(), remaining);
    set_hidden_value(stream, hidden_key(b"readableLength"), remaining);
    sync_pending_readable_chunks_to_buffer(stream);
}

fn read_stream_exact_bytes(stream: f64, requested: usize) -> f64 {
    let mut values = Vec::new();
    if let Some(chunks) = readable_hidden_chunks(stream) {
        push_chunk_values(chunks, &mut values, 0);
    }
    if values.is_empty() {
        return f64::from_bits(TAG_NULL);
    }

    let mut consumed = Vec::new();
    let mut remaining_values = Vec::new();
    let mut remaining_len = 0usize;
    let mut needed = requested;

    for value in values {
        let mut bytes = Vec::new();
        append_chunk_bytes(value, &mut bytes, 0);
        if needed == 0 {
            remaining_len += bytes.len();
            remaining_values.push(value);
            continue;
        }
        if bytes.len() <= needed {
            consumed.extend_from_slice(&bytes);
            needed -= bytes.len();
            continue;
        }

        consumed.extend_from_slice(&bytes[..needed]);
        let rest = &bytes[needed..];
        if !rest.is_empty() {
            remaining_len += rest.len();
            remaining_values.push(buffer_value_from_bytes(rest));
        }
        needed = 0;
    }

    if consumed.is_empty() || needed > 0 {
        return f64::from_bits(TAG_NULL);
    }

    set_readable_buffer_values(stream, &remaining_values, remaining_len);
    mark_disturbed(stream);
    if stream_hidden_ended(stream) && remaining_len == 0 {
        clear_pending_readable_chunks(stream);
        queue_readable_event(stream);
        schedule_readable_end(stream);
    }
    buffer_value_from_bytes(&consumed)
}

pub(super) fn buffer_value_from_bytes(bytes: &[u8]) -> f64 {
    let buf = crate::buffer::js_buffer_alloc(bytes.len() as i32, 0);
    if !bytes.is_empty() {
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                crate::buffer::buffer_data_mut(buf),
                bytes.len(),
            );
        }
    }
    box_pointer(buf as *const u8)
}

pub(super) fn read_stream_object_mode_chunk(stream: f64) -> f64 {
    let Some(chunks) = readable_hidden_chunks(stream) else {
        return f64::from_bits(TAG_NULL);
    };
    if !is_array_like_value(chunks) {
        clear_readable_buffer(stream);
        return chunks;
    }
    let arr = raw_ptr_from_value(chunks) as *mut crate::array::ArrayHeader;
    if crate::array::js_array_length(arr) == 0 {
        clear_readable_buffer(stream);
        return f64::from_bits(TAG_NULL);
    }
    let chunk = crate::array::js_array_shift_f64(arr);
    let remaining = crate::array::js_array_length(arr) as f64;
    set_hidden_value(stream, hidden_buffered_key(), remaining);
    set_hidden_value(stream, hidden_key(b"readableLength"), remaining);
    mark_disturbed(stream);
    sync_pending_readable_chunks_to_buffer(stream);
    if stream_hidden_ended(stream) && remaining == 0.0 {
        clear_pending_readable_chunks(stream);
        queue_readable_event(stream);
        schedule_readable_end(stream);
    }
    chunk
}

pub(super) fn string_chunk_to_buffer(value: f64) -> Option<f64> {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_any_string() {
        return None;
    }
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const crate::StringHeader;
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return None;
    }
    Some(box_pointer(
        crate::buffer::js_buffer_from_string(ptr, 0) as *const u8
    ))
}
