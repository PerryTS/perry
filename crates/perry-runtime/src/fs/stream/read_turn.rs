//! Coalesced native turns for file-backed Readable reads.

use super::*;

/// A read runs after the caller has attached its chained listeners. Coalesce
/// repeated requests until the queued native FS callback takes its turn.
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
        let closure = js_closure_alloc(
            crate::fn_info!(read_stream_turn_impl, 0; with_declared(0)),
            1,
        );
        crate::closure::closure_set_capture_word(closure, 0, id as i64);
        let _ = crate::timer::schedule_native_timeout(closure as i64, 0.0, "FSREQCALLBACK");
    }
}

pub(super) extern "C" fn read_stream_turn_impl(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let id = stream_id_of(closure);
    STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.turn_pending = false;
        }
    });
    read_stream_deliver_impl(closure, _this);
    undefined_value()
}
