//! node 26.12's message-level cork on a `ServerResponse` (nodejs/node
//! f9175a212c, PR 64987): the cork count and the chunk buffer live on the
//! response state; `uncork()` sends the buffered chunked writes as one chunk.

use perry_ffi::RawClosureHeader;

use crate::server::request::handle_to_pointer_f64;
use crate::server::response::{begin_streaming_head, response_state, response_state_mut};
use crate::server::types::{PTR_MASK, TAG_UNDEFINED};

extern "C" {
    fn js_buffer_is_encoding(value: f64) -> i32;
}

/// node f9175a212c (`write_`): a string chunk written to a chunked message
/// with an encoding `Buffer.isEncoding` rejects throws `ERR_UNKNOWN_ENCODING`.
/// The head is prepared first (node's `_implicitHeader()`), since the framing
/// decides whether the message is chunked; it stays unsent. An ended or
/// destroyed message answers the write with its own error first (node).
pub(crate) fn throw_unknown_chunked_encoding(handle: i64, chunk: f64, encoding_bits: i64) {
    let encoding = f64::from_bits(encoding_bits as u64);
    if response_state(handle).is_none_or(|sr| sr.writable_ended || sr.destroyed)
        || !crate::is_string_value(chunk)
        || !crate::is_string_value(encoding)
        || unsafe { js_buffer_is_encoding(encoding) } != 0
    {
        return;
    }
    // SAFETY: `encoding` is a NaN-boxed string (checked above).
    let name = unsafe { crate::extract_string_value(encoding) }.unwrap_or_default();
    if name.is_empty() || !begin_streaming_head(handle, true) {
        return;
    }
    let Some((conn, seq)) = response_state(handle).and_then(|sr| sr.turnloop) else {
        return;
    };
    if crate::server::turnloop_route::http1_chunked(
        &conn.root(crate::server::response::response_socket(handle)),
        seq,
    ) == Some(true)
    {
        perry_ffi::throw_with_code(
            &format!("Unknown encoding: {name}"),
            "ERR_UNKNOWN_ENCODING",
            perry_ffi::ErrorKind::TypeError,
        );
    }
}

/// `res.cork()`: node's `this[kCorked]++`.
#[no_mangle]
pub extern "C" fn js_node_http_res_cork(handle: i64) {
    if let Some(sr) = response_state_mut(handle) {
        sr.corked += 1;
    }
}

/// `res.uncork()`: node's `this[kCorked]--`, then the buffered chunked writes
/// go out as one chunk once the count is zero (node f9175a212c).
#[no_mangle]
pub extern "C" fn js_node_http_res_uncork(handle: i64) {
    let Some(sr) = response_state_mut(handle) else {
        return;
    };
    sr.corked -= 1;
    if sr.corked == 0 {
        flush_chunked_buffer(handle);
    }
}

/// node's `OutgoingMessage.writableLength` (f9175a212c) without the socket
/// term: the buffered body, plus the buffered chunk with its framing and,
/// while it is not sent, the head. node's `socket.writableLength` counts what
/// a synchronous write left unsent; perry's connection queues every write
/// until the loop flushes it, so its queued count is not that number.
pub(crate) fn writable_length(handle: i64) -> usize {
    let Some(sr) = response_state(handle) else {
        return 0;
    };
    let mut len = sr.buffered_body.len();
    if !sr.chunked_buffer.is_empty() {
        len += chunked_buffer_wire_len(sr.chunked_buffer.len());
        if let Some((conn, seq)) = sr.turnloop {
            len += crate::server::turnloop_route::unsent_head_len(
                &conn.root(crate::server::response::response_socket(handle)),
                seq,
            );
        }
    }
    len
}

/// The bytes a buffered chunk adds to node's `writableLength`: the data, its
/// hex length line and the two CRLFs (node f9175a212c).
pub(crate) fn chunked_buffer_wire_len(len: usize) -> usize {
    if len == 0 {
        0
    } else {
        len + format!("{len:x}").len() + 4
    }
}

/// Send the bytes collected while the message was corked as one chunk. A
/// destroyed response drops them, as node's `uncork()` does.
pub(crate) fn flush_chunked_buffer(handle: i64) {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    let Some(sr) = response_state_mut(response_root.get()) else {
        return;
    };
    let bytes = std::mem::take(&mut sr.chunked_buffer);
    if bytes.is_empty() || sr.destroyed {
        return;
    }
    let Some((conn, seq)) = sr.turnloop else {
        return;
    };
    let _ = crate::server::turnloop_route::send_body(
        &conn.root(crate::server::response::response_socket(
            response_root.get(),
        )),
        seq,
        &bytes,
    );
}

extern "C" {
    fn js_queue_next_tick(callback: i64);
}

/// node's `process.nextTick(connectionCorkNT, msg)`.
pub(crate) fn queue_connection_cork_tick(handle: i64) {
    let scope = perry_ffi::TransientRootScope::enter();
    let response = scope.root_nanbox(handle_to_pointer_f64(handle));
    let job = scope.root_addr(perry_ffi::alloc_closure(
        perry_ffi::js_function_info!(connection_cork_tick, 0; with_flags(perry_ffi::FN_BUILTIN)),
        1,
    ) as i64);
    // SAFETY: the rooted closure has one traced capture slot; the response
    // value is reloaded from its root after the allocation.
    unsafe {
        perry_ffi::set_closure_capture_f64(job.get() as *mut RawClosureHeader, 0, response.get());
        js_queue_next_tick(job.get());
    }
}

/// node's `connectionCorkNT(msg)`: `if (msg[kCorked]) msg.uncork()`.
unsafe extern "C" fn connection_cork_tick(
    closure: *const RawClosureHeader,
    _: perry_ffi::JsThis,
) -> f64 {
    let handle = (perry_ffi::closure_capture_f64(closure, 0).to_bits() & PTR_MASK) as i64;
    if response_state(handle).is_some_and(|sr| sr.corked != 0) {
        js_node_http_res_uncork(handle);
    }
    f64::from_bits(TAG_UNDEFINED)
}
