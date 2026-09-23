//! node:stream — live-emit consume helpers (split out of
//! node_stream_readwrite.rs for the 2000-line file-size gate).
//!
//! A chunk that `push()`/`unshift()` hands straight to a flowing reader must
//! also leave the retained readable buffer. Otherwise the buffer keeps a copy
//! of an already-delivered chunk, and the next resume/drain microtask replays
//! it — duplicating every chunk written in the same tick as `on('data')`,
//! not just the `pipe()`/`pipeline()`/`compose()` chains this used to be
//! limited to (#11045).
//!
//! A readable fed by live `push()` output is also marked, so the drain knows
//! the retained buffer is not a finite snapshot (`Readable.from`, a duplex
//! built from a source): running it dry is not end-of-stream — only EOF
//! (`push(null)`, a finished writable side) is.
use super::*;

pub(super) fn consume_readable_buffered_front_on_live_emit(stream: f64, chunk: f64) {
    super::readable_from_promises::consume_readable_buffered_front(stream, chunk);
}

pub(super) fn mark_readable_live_push(stream: f64) {
    if !has_truthy_hidden(stream, hidden_readable_live_push_key()) {
        set_hidden_value(
            stream,
            hidden_readable_live_push_key(),
            f64::from_bits(TAG_TRUE),
        );
    }
}

/// Whether draining the retained buffer may emit `'end'`: a snapshot-backed
/// readable ends once drained, a live-push one only after EOF.
pub(super) fn readable_drain_may_end(stream: f64) -> bool {
    stream_hidden_ended(stream) || !has_truthy_hidden(stream, hidden_readable_live_push_key())
}
