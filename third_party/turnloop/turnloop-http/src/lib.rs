//! Sans-I/O HTTP codecs and host-driven client policy. No socket, executor or clock reads.
//!
//! # Getting started on turnloop
//! Enable the `turnloop` feature for the `asynchronous` module. The embedding
//! host owns `LocalExecutor` and calls `turn`; adapters await its streams and
//! deadline futures. See the crate README and turnloop-io for ownership, streaming
//! and cancellation examples. Default features retain the sans-I/O API.
//!
//! # The `Step` contract
//!
//! Both decoders here - [`http1::Decoder`] and [`http2::Connection`] - are
//! driven the same way: call `receive` with everything you have, act on the step
//! it returns, drain `consumed`, repeat. **`consumed` and `event` are
//! independent, and a host has to look at both.** The rule is one line:
//!
//! ```text
//! progressed = step.consumed > 0 || step.event.is_some()
//! ```
//!
//! Call `receive` again while `progressed` is true. When it is false, read more
//! bytes from the transport - or declare the end of input with `eof` - before
//! calling again. Each half of that condition has cost a host a debugging cycle,
//! so neither is optional:
//!
//! * **`consumed > 0` with no event** is progress with nothing to hand up: the
//!   HTTP/2 client preface, a PRIORITY frame, an
//!   unknown frame type, or a frame the peer had in flight for a stream that is
//!   already gone; an HTTP/1 chunk-size line, chunk CRLF or empty trailer
//!   block. A loop that continues only while an event came back stalls here, and
//!   for HTTP/2 the first such step is the preface - so the connection never
//!   starts at all.
//! * **`consumed == 0` with an event** is an event that reads no input.
//!   HTTP/1's [`http1::Event::End`] and [`http1::Event::Upgrade`] both arrive
//!   this way. A loop that continues only while input was consumed - or only
//!   while it has input to feed - drops the end of every message;
//!   [`http1::Decoder::wants_step`] says when such an event is pending. HTTP/2
//!   has no step of this shape: there an event always
//!   consumes, and `consumed == 0` is always the stop case.
//! * **`consumed == 0` with no event** is the only stop condition, and it is
//!   returned whether the decoder needs more bytes or is finished for good. The
//!   two are not distinguishable from the step: a host that has seen
//!   [`http1::Event::End`] must remember it (or ask
//!   [`http1::Decoder::reusable`]), and an HTTP/2 host closes on
//!   [`http2::Connection::is_drained`].
//!
//! `receive` is not idempotent: it advances decoder state by `consumed`, so the
//! host must drain exactly that many bytes before calling again. Feeding the
//! same input back - which is what a stalled loop does when it retries - fails
//! the connection.
//!
//! `asynchronous::Http1::event` and `asynchronous::Http2::event` are the
//! reference drivers, and `Step`'s own documentation carries the per-decoder
//! table.
#![deny(unsafe_op_in_unsafe_fn)]
pub mod client;
pub mod compression;
pub mod hpack;
pub mod http1;
pub mod http2;
pub mod multipart;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error {
    pub code: &'static str,
    pub message: &'static str,
}
impl Error {
    pub const fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

mod recycling;

#[cfg(feature = "turnloop")]
pub mod asynchronous;
