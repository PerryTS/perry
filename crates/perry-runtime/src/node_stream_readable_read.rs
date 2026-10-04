//! Readable `read()` consumption helpers, split from node_stream_readwrite.rs.
use super::*;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};

/// Test seam marking an allocation inside a window that holds JS values: a
/// test installs a hook that collects here, standing in for a collection
/// triggered by that allocation. Compiles to nothing outside tests.
#[inline(always)]
pub(super) fn allocation_point() {
    #[cfg(test)]
    rooted_gc_tests::at_allocation_point();
}

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
    // `_read` is user code and can collect.
    let scope = RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    invoke_read_once(stream.get_nanbox_f64());
    read_stream_available_default(stream.get_nanbox_f64())
}

/// The byte length of `value`'s chunk bytes.
fn chunk_byte_len(value: f64) -> usize {
    let mut bytes = Vec::new();
    append_chunk_bytes(value, &mut bytes, 0);
    bytes.len()
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

    // Rebuilding the buffer and decoding allocate, so the stream and the
    // buffered chunks are held in handles (a rooted GC array, never a Rust
    // `Vec` the collector cannot see).
    let scope = RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let s = || stream.get_nanbox_f64();

    // `read()` with no size argument mirrors Node's `howMuchToRead(NaN)`:
    // a FLOWING stream consumes ONE chunk (the buffer head) so 'data'
    // emission preserves chunk boundaries, while a paused stream drains the
    // entire internal buffer and returns it as a single value — Node only
    // takes `state.buffer.first()` when `state.flowing && state.length`.
    // Sized `read(n)` (read_stream_exact_size) still spans chunks.
    // (#1545, #2484)
    let values = match readable_hidden_chunks(s()) {
        Some(chunks) => chunk_values_snapshot(&scope, chunks),
        None => rooted_empty_array(&scope),
    };
    let len = rooted_array_len(&values);
    if len == 0 {
        if stream_hidden_ended(s()) {
            cancel_readable_event(s());
            refresh_readable_aborted_flag(s());
        }
        return f64::from_bits(TAG_NULL);
    }

    // Node's `howMuchToRead(NaN)` (internal/streams/readable, v26): WITHOUT a
    // string decoder, `read()` always returns just the head chunk — one buffer
    // at a time — even when the stream is paused. Only a *decoded* (setEncoding)
    // paused stream concatenates the entire buffer into a single string. So we
    // drain the whole buffer here only when paused AND a decoder is active;
    // otherwise fall through to the head-only path below.
    if !readable_is_flowing(s()) && readable_encoding_tag(s()).is_some() {
        return drain_whole_buffer(&stream, &values);
    }

    let head = scope.root_nanbox_f64(rooted_array_at(&values, 0));
    let remaining_len: usize = (1..len)
        .map(|i| chunk_byte_len(rooted_array_at(&values, i)))
        .sum();
    let remaining = rooted_array_tail(&scope, &values, 1);
    set_readable_buffer_values(&stream, &remaining, remaining_len);
    mark_disturbed(s());
    if stream_hidden_ended(s()) && remaining_len == 0 {
        clear_pending_readable_chunks(s());
        queue_readable_event(s());
        schedule_readable_end(s());
    }

    if readable_encoding_tag(s()).is_some() {
        return super::decode_readable_chunk_for_encoding(s(), head.get_nanbox_f64())
            .unwrap_or(f64::from_bits(TAG_NULL));
    }

    let mut bytes = Vec::new();
    append_chunk_bytes(head.get_nanbox_f64(), &mut bytes, 0);
    buffer_value_from_bytes(&bytes)
}

/// Elements `from..` of the rooted array `values`, copied into a new GC array
/// rooted in `scope`.
fn rooted_array_tail<'s>(
    scope: &'s RuntimeHandleScope,
    values: &RuntimeHandle<'_>,
    from: u32,
) -> RuntimeHandle<'s> {
    let len = rooted_array_len(values);
    let tail = scope.root_nanbox_f64(box_pointer(crate::array::js_array_alloc(
        len.saturating_sub(from),
    ) as *const u8));
    for i in from..len {
        rooted_array_push(&tail, rooted_array_at(values, i));
    }
    tail
}

/// Paused-mode `read()` with no size: consume every buffered chunk and return
/// them as one concatenated value (Node's `howMuchToRead(NaN)` returns
/// `state.length` when the stream is not flowing). Clearing the buffer leaves
/// `values` the only holder of the chunks, so it must be a rooted GC array.
fn drain_whole_buffer(stream: &RuntimeHandle<'_>, values: &RuntimeHandle<'_>) -> f64 {
    let s = || stream.get_nanbox_f64();
    clear_readable_buffer(s());
    mark_disturbed(s());
    clear_pending_readable_chunks(s());
    if stream_hidden_ended(s()) {
        queue_readable_event(s());
        schedule_readable_end(s());
    }

    if readable_encoding_tag(s()).is_some() {
        let scope = RuntimeHandleScope::new();
        let decoded = rooted_empty_array(&scope);
        for i in 0..rooted_array_len(values) {
            allocation_point();
            if let Some(value) =
                super::decode_readable_chunk_for_encoding(s(), rooted_array_at(values, i))
            {
                rooted_array_push(&decoded, value);
            }
        }
        let count = rooted_array_len(&decoded);
        if count == 0 {
            return f64::from_bits(TAG_NULL);
        }
        if count == 1 {
            return rooted_array_at(&decoded, 0);
        }
        // `js_string_concat_chain` roots its parts before it allocates.
        let parts: Vec<f64> = (0..count).map(|i| rooted_array_at(&decoded, i)).collect();
        let result = crate::string::js_string_concat_chain(parts.as_ptr(), parts.len() as i32);
        return f64::from_bits(JSValue::string_ptr(result).bits());
    }

    let mut bytes = Vec::new();
    for i in 0..rooted_array_len(values) {
        append_chunk_bytes(rooted_array_at(values, i), &mut bytes, 0);
    }
    buffer_value_from_bytes(&bytes)
}

pub(super) fn read_stream_exact_size(stream: f64, size: f64) -> f64 {
    // `_read` is user code and can collect.
    let scope = RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let s = || stream.get_nanbox_f64();
    invoke_read_once(s());
    if size <= 0.0 {
        return f64::from_bits(TAG_NULL);
    }
    let requested = size as usize;
    let available = get_hidden_value(s(), hidden_buffered_key())
        .unwrap_or(0.0)
        .max(0.0) as usize;
    if available == 0 {
        if stream_hidden_ended(s()) {
            cancel_readable_event(s());
            refresh_readable_aborted_flag(s());
        }
        return f64::from_bits(TAG_NULL);
    }
    if readable_encoding_tag(s()).is_some() {
        return read_stream_available_default(s());
    }
    if requested > available && !stream_hidden_ended(s()) {
        return f64::from_bits(TAG_NULL);
    }
    if requested >= available {
        return read_stream_exact_bytes(&stream, available);
    }

    read_stream_exact_bytes(&stream, requested)
}

fn sync_pending_readable_chunks_to_buffer(stream: &RuntimeHandle<'_>) {
    let scope = RuntimeHandleScope::new();
    let pending = rooted_empty_array(&scope);
    if let Some(chunks) = readable_hidden_chunks(stream.get_nanbox_f64()) {
        let chunks = scope.root_nanbox_f64(chunks);
        if is_array_like_value(chunks.get_nanbox_f64()) {
            for i in 0..rooted_array_len(&chunks) {
                rooted_array_push(&pending, rooted_array_at(&chunks, i));
            }
        } else if is_single_chunk_value(chunks.get_nanbox_f64()) {
            rooted_array_push(&pending, chunks.get_nanbox_f64());
        }
    }
    set_hidden_value(
        stream.get_nanbox_f64(),
        hidden_readable_pending_key(),
        pending.get_nanbox_f64(),
    );
}

/// Make the rooted array `values` (owned by the caller, not shared) the
/// stream's buffer, holding `byte_len` bytes.
fn set_readable_buffer_values(
    stream: &RuntimeHandle<'_>,
    values: &RuntimeHandle<'_>,
    byte_len: usize,
) {
    if rooted_array_len(values) == 0 || byte_len == 0 {
        clear_readable_buffer(stream.get_nanbox_f64());
        sync_pending_readable_chunks_to_buffer(stream);
        return;
    }
    set_hidden_value(
        stream.get_nanbox_f64(),
        hidden_chunks_key(),
        values.get_nanbox_f64(),
    );
    let remaining = byte_len as f64;
    set_hidden_value(stream.get_nanbox_f64(), hidden_buffered_key(), remaining);
    set_hidden_value(
        stream.get_nanbox_f64(),
        hidden_key(b"readableLength"),
        remaining,
    );
    sync_pending_readable_chunks_to_buffer(stream);
}

fn read_stream_exact_bytes(stream: &RuntimeHandle<'_>, requested: usize) -> f64 {
    let s = || stream.get_nanbox_f64();
    let scope = RuntimeHandleScope::new();
    let values = match readable_hidden_chunks(s()) {
        Some(chunks) => chunk_values_snapshot(&scope, chunks),
        None => return f64::from_bits(TAG_NULL),
    };
    if rooted_array_len(&values) == 0 {
        return f64::from_bits(TAG_NULL);
    }

    let mut consumed = Vec::new();
    let remaining_values = rooted_empty_array(&scope);
    let mut remaining_len = 0usize;
    let mut needed = requested;

    for i in 0..rooted_array_len(&values) {
        let mut bytes = Vec::new();
        append_chunk_bytes(rooted_array_at(&values, i), &mut bytes, 0);
        if needed == 0 {
            remaining_len += bytes.len();
            rooted_array_push(&remaining_values, rooted_array_at(&values, i));
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
            allocation_point();
            let rest = buffer_value_from_bytes(rest);
            rooted_array_push(&remaining_values, rest);
        }
        needed = 0;
    }

    if consumed.is_empty() || needed > 0 {
        return f64::from_bits(TAG_NULL);
    }

    set_readable_buffer_values(stream, &remaining_values, remaining_len);
    mark_disturbed(s());
    if stream_hidden_ended(s()) && remaining_len == 0 {
        clear_pending_readable_chunks(s());
        queue_readable_event(s());
        schedule_readable_end(s());
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
    let remaining = (crate::array::js_array_length(arr) - 1) as f64;
    // The bookkeeping below allocates; hold the stream and the chunk.
    let scope = RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let chunk = scope.root_nanbox_f64(crate::array::js_array_shift_f64(arr));
    let s = || stream.get_nanbox_f64();
    set_hidden_value(s(), hidden_buffered_key(), remaining);
    set_hidden_value(s(), hidden_key(b"readableLength"), remaining);
    mark_disturbed(s());
    sync_pending_readable_chunks_to_buffer(&stream);
    if stream_hidden_ended(s()) && remaining == 0.0 {
        clear_pending_readable_chunks(s());
        // Node never emits a final `readable` just to hand the consumer a
        // null at EOF: once `read()` returns the last buffered item from an
        // ended stream it transitions straight to `end` (endReadable). A
        // re-queued `readable` here makes a fixed-count consumer observe an
        // extra `read() === null` pair. (internal/streams/readable, v26)
        schedule_readable_end(s());
    }
    chunk.get_nanbox_f64()
}

#[cfg(test)]
#[path = "node_stream_rooted_gc_tests.rs"]
mod rooted_gc_tests;
