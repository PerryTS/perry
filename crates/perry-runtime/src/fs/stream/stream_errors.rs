//! A stream's stored `'error'` value and its delivery, split out of
//! `stream.rs` to keep it under the 2000-line cap.

use super::*;

pub(super) fn make_error_value(message: &str) -> f64 {
    let msg = message.as_bytes();
    let err_str = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_obj = crate::error::js_error_new_with_message(err_str);
    crate::value::js_nanbox_pointer(err_obj as i64)
}

/// The stream's stored error as a JS value: the node-shaped value the deferred
/// open produced when there is one (#9493), else an `Error` over `error_msg`.
pub(super) fn stored_error_value(state: &StreamState) -> Option<f64> {
    if !JSValue::from_bits(state.error_value.to_bits()).is_undefined() {
        return Some(state.error_value);
    }
    state.error_msg.as_deref().map(make_error_value)
}

pub(super) fn emit_stored_error(id: usize) {
    let error_value = STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        registry.get(&id).and_then(stored_error_value)
    });
    if let Some(err) = error_value {
        emit_event1(id, "error", err);
    }
}

pub(super) fn record_stream_error(id: usize, message: String) {
    STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.errored = true;
            state.error_msg = Some(message);
        }
    });
    refresh_props(id);
    emit_stored_error(id);
}

/// #10451: a read stream's failure as Node reports it. `error_msg` alone
/// became a bare `Error` with the Rust text ("No such file or directory (os
/// error 2)") and no `code`/`errno`/`syscall`/`path`, while the write side
/// already stored a node-shaped `error_value` (#9493).
fn store_read_failure(id: usize, failure: &FsReadFailure) {
    let error_value = unsafe { failure.error_value() };
    // No allocation between building the value and storing it: from here the
    // registry is the GC root that keeps it (and rewrites it if it moves).
    STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.error_value = error_value;
        }
    });
}

/// Turn the constructor's open failure into `error_value` once the state is
/// registered, so the `'error'` replay to a listener attached right after
/// construction and the pump's delivery both hand out the node-shaped value.
pub(super) fn store_open_failure(id: usize) {
    let failure = STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        registry
            .get_mut(&id)
            .and_then(|state| state.open_failure.take())
    });
    if let Some(failure) = failure {
        store_read_failure(id, &failure);
    }
}

/// The node-shaped error a read or write stream has already stored, if any.
/// Unlike `stored_error_value` this never synthesizes a bare `Error` from
/// `error_msg`: the caller wants the failure's `code`/`syscall`/`path` or
/// nothing.
pub(super) fn stored_node_error_value(id: usize) -> Option<f64> {
    STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let state = registry.get(&id)?;
        (!JSValue::from_bits(state.error_value.to_bits()).is_undefined())
            .then_some(state.error_value)
    })
}

/// A failed read while pumping (`EISDIR ... read` for a directory).
pub(super) fn record_read_failure(id: usize, failure: FsReadFailure) {
    store_read_failure(id, &failure);
    record_stream_error(id, failure.err.to_string());
}

/// Starting a ReadStream must deliver a constructor-time open failure. Without
/// this transition, event-backed consumers wait forever for data/end/error
/// after `createReadStream()` recorded an invalid path (#9616).
pub(super) fn emit_pending_read_error(id: usize) -> bool {
    let pending = STREAM_REGISTRY.with(|registry| {
        registry.borrow().get(&id).and_then(|state| {
            (state.kind == StreamKind::Read && !state.errored)
                .then(|| state.error_msg.clone())
                .flatten()
        })
    });
    let Some(message) = pending else {
        return false;
    };
    record_stream_error(id, message);
    maybe_close_stream(id, false);
    true
}
