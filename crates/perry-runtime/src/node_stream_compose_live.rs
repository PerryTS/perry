//! node:stream — flowing-readable consume helper (split out of
//! node_stream_readwrite.rs for the 2000-line file-size gate).

pub(super) fn consume_readable_buffered_front_on_emit(stream: f64, chunk: f64) {
    super::readable_from_promises::consume_readable_buffered_front(stream, chunk);
}
