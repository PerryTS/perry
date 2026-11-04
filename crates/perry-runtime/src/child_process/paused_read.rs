//! Paused-mode reading for a child's `stdout`/`stderr`/`stdio[n]` pipes. Attaching a
//! `readable` listener switches the stream to paused mode: incoming bytes accumulate on
//! the stream, `read()` hands them out, and `end` follows the read that drains them.

use super::*;

use crate::value::JSValue;

use crate::closure::{
    js_closure_get_capture_ptr, js_closure_set_capture_ptr, ClosureHeader, JsThis,
};

const PAUSED_KEY: &[u8] = b"__cpPaused";
const BUFFER_KEY: &[u8] = b"__cpBuf";
const EOF_KEY: &[u8] = b"__cpEof";
const ENDED_KEY: &[u8] = b"__cpEnded";
const ENCODING_KEY: &[u8] = b"__cpEncoding";

fn flag(stream: f64, key: &[u8]) -> bool {
    cp_get_field(stream, key).to_bits() == TAG_TRUE_F64.to_bits()
}

fn buffered(stream: f64) -> Vec<u8> {
    let value = cp_get_field(stream, BUFFER_KEY);
    if JSValue::from_bits(value.to_bits()).is_undefined() {
        Vec::new()
    } else {
        cp_value_to_bytes(value)
    }
}

fn store_buffered(stream: f64, bytes: &[u8]) {
    let value = if bytes.is_empty() {
        cp_undefined()
    } else {
        cp_make_buffer(bytes)
    };
    cp_set_field(stream, BUFFER_KEY, value);
}

/// A Buffer, or a string once `setEncoding` was called.
fn box_chunk(stream: f64, bytes: &[u8]) -> f64 {
    match cp_value_to_string(cp_get_field(stream, ENCODING_KEY)) {
        Some(encoding) => cp_box_output(bytes, &CpOutput::Text(encoding)),
        None => cp_make_buffer(bytes),
    }
}

/// Hand bytes read from the pipe to the stream: buffered in paused mode, emitted as
/// `data` otherwise.
pub(crate) fn cp_stream_deliver(stream: f64, bytes: &[u8]) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    if flag(stream.get_nanbox_f64(), PAUSED_KEY) {
        let mut all = buffered(stream.get_nanbox_f64());
        all.extend_from_slice(bytes);
        store_buffered(stream.get_nanbox_f64(), &all);
        cp_emit(stream.get_nanbox_f64(), "readable", &[]);
    } else {
        let chunk = scope.root_nanbox_f64(box_chunk(stream.get_nanbox_f64(), bytes));
        cp_emit(stream.get_nanbox_f64(), "data", &[chunk.get_nanbox_f64()]);
    }
}

/// Pipe EOF on a paused stream: signal `readable` once more and keep `end` back until the
/// buffered bytes are read. Returns false when the stream is not in paused mode.
pub(crate) fn cp_paused_eof(stream: f64) -> bool {
    if !flag(stream, PAUSED_KEY) {
        return false;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    cp_set_field(stream.get_nanbox_f64(), EOF_KEY, TAG_TRUE_F64);
    cp_emit(stream.get_nanbox_f64(), "readable", &[]);
    end_if_drained(stream.get_nanbox_f64());
    true
}

fn end_if_drained(stream: f64) {
    if flag(stream, EOF_KEY) && !flag(stream, ENDED_KEY) && buffered(stream).is_empty() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let stream = scope.root_nanbox_f64(stream);
        cp_set_field(stream.get_nanbox_f64(), ENDED_KEY, TAG_TRUE_F64);
        super::emitter::cp_readable_end_now(stream.get_nanbox_f64());
    }
}

/// Called after every listener registration: a first `readable` listener puts the stream in
/// paused mode and announces anything already waiting.
pub(crate) fn cp_listener_added(target: f64, event: &str) {
    if event != "readable"
        || JSValue::from_bits(cp_get_field(target, b"readable").to_bits()).is_undefined()
    {
        return;
    }
    cp_set_field(target, PAUSED_KEY, TAG_TRUE_F64);
    if flag(target, EOF_KEY) || !buffered(target).is_empty() {
        schedule_settle(target);
    }
}

fn schedule_settle(stream: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let deferred = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(cp_settle_thunk, 0; with_declared(0)),
        1,
    ));
    deferred.with_mut_ptr(|deferred: *mut ClosureHeader| {
        js_closure_set_capture_ptr(deferred, 0, stream.get_nanbox_f64().to_bits() as i64);
    });
    deferred.with_mut_ptr(|deferred: *mut ClosureHeader| {
        crate::timer::js_set_immediate_callback(deferred as i64);
    });
}

extern "C" fn cp_settle_thunk(closure: *const ClosureHeader, _this: JsThis) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream =
        scope.root_nanbox_f64(f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64));
    if flag(stream.get_nanbox_f64(), PAUSED_KEY) {
        if !buffered(stream.get_nanbox_f64()).is_empty() {
            cp_emit(stream.get_nanbox_f64(), "readable", &[]);
        }
        end_if_drained(stream.get_nanbox_f64());
    }
    cp_undefined()
}

/// `readable.read([size])`: the buffered bytes (all of them without `size`), or null when
/// there are not enough yet. Without a `readable` listener nothing is buffered, so null.
pub(crate) extern "C" fn cp_method_read(
    closure: *const ClosureHeader,
    this: JsThis,
    size: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(cp_this(this, closure));
    if !flag(stream.get_nanbox_f64(), PAUSED_KEY) {
        return TAG_NULL_F64;
    }
    let all = buffered(stream.get_nanbox_f64());
    let eof = flag(stream.get_nanbox_f64(), EOF_KEY);
    let take = if size.is_finite() && size >= 0.0 {
        let size = size as usize;
        if size == 0 || (size > all.len() && !eof) {
            return TAG_NULL_F64;
        }
        size.min(all.len())
    } else {
        all.len()
    };
    if take == 0 {
        if eof {
            schedule_settle(stream.get_nanbox_f64());
        }
        return TAG_NULL_F64;
    }
    store_buffered(stream.get_nanbox_f64(), &all[take..]);
    let chunk = scope.root_nanbox_f64(box_chunk(stream.get_nanbox_f64(), &all[..take]));
    if eof && take == all.len() {
        schedule_settle(stream.get_nanbox_f64());
    }
    cp_emit(stream.get_nanbox_f64(), "data", &[chunk.get_nanbox_f64()]);
    chunk.get_nanbox_f64()
}
