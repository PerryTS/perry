//! node:stream — live-pipe consume helpers (split out of
//! node_stream_readwrite.rs for the 2000-line file-size gate). These mark a
//! readable as a live pipe participant and consume its buffered front when it
//! re-emits, so `pipe()`, `pipeline()`, and `compose()` chains don't replay the
//! same flowing chunk from retained readable storage.
use super::*;

pub(super) fn mark_live_pipe_consume_on_emit(stream: f64) {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_some() {
        set_hidden_value(
            stream,
            hidden_compose_live_pipe_consume_key(),
            f64::from_bits(TAG_TRUE),
        );
    }
}

pub(super) fn consume_readable_buffered_front_for_live_pipe(stream: f64, chunk: f64) {
    if has_truthy_hidden(stream, hidden_compose_live_pipe_consume_key()) {
        super::readable_from_promises::consume_readable_buffered_front(stream, chunk);
    }
}
