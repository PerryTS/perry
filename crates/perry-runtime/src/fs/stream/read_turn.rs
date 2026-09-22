//! Deferred `ReadStream` start and error-close turns.

use super::*;

/// Start a readable on a later event-loop turn. Node opens file streams
/// asynchronously, so a caller must be able to finish a chained sequence such
/// as `.on("data", ...).on("error", ...)` before an open failure is emitted.
/// The pending bit coalesces starts from construction, `on("data")`, `pipe`,
/// and `resume` into one callback.
pub(super) fn schedule_read_stream_turn(id: usize) {
    let should_schedule = STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(state) = registry.get_mut(&id) else {
            return false;
        };
        if state.turn_pending || state.closed || state.destroyed {
            return false;
        }
        state.turn_pending = true;
        true
    });
    if should_schedule {
        let closure = js_closure_alloc(read_stream_turn_impl as *const u8, 1);
        js_closure_set_capture_ptr(closure, 0, id as i64);
        let _ = crate::timer::js_set_timeout_callback(closure as i64, 0.0);
    }
}

pub(super) extern "C" fn read_stream_turn_impl(closure: *const ClosureHeader) -> f64 {
    let id = stream_id_of(closure);
    let should_close = STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.turn_pending = false;
            state.kind == StreamKind::Read && state.errored && !state.closed
        } else {
            false
        }
    });
    if should_close {
        maybe_close_stream(id, false);
    } else {
        read_stream_pump(id);
    }
    undefined_value()
}
