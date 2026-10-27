//! Bounded owned-byte exchange between a guest's WASI stream and native HTTP.
//! No Store, guest pointer, callback or ambient executor crosses this boundary.
mod incoming;
#[cfg(test)]
mod tests;
pub use incoming::{FutureTrailers, IncomingBody, IncomingBodySender};

use super::{Error, FieldMap};
use bytes::Bytes;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use wasmtime_wasi_io::{
    poll::Pollable,
    streams::{OutputStream, StreamError},
};

pub const CHUNK_SIZE: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamContext {
    Request,
    Response,
}
impl StreamContext {
    fn size_error(self, actual: Option<u64>) -> Error {
        match self {
            Self::Request => Error::HttpRequestBodySize(actual),
            Self::Response => Error::HttpResponseBodySize(actual),
        }
    }
}

type Completion = Result<Option<FieldMap>, Error>;
struct State {
    chunk: Option<Bytes>,
    end: Option<Completion>,
    written: u64,
    expected: Option<u64>,
    context: StreamContext,
    consumer_alive: bool,
    stream_alive: bool,
    discard: bool,
    reader: Option<Waker>,
    writer: Option<Waker>,
}
struct Shared {
    state: Mutex<State>,
}
impl Shared {
    fn new(context: StreamContext, expected: Option<u64>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                chunk: None,
                end: None,
                written: 0,
                expected,
                context,
                consumer_alive: true,
                stream_alive: true,
                discard: false,
                reader: None,
                writer: None,
            }),
        })
    }
    // Wake outside the lock: a waker is arbitrary caller code and may poll
    // immediately on another thread. State checks and registration share a lock.
    fn wake(&self) {
        let (reader, writer) = {
            let mut state = self.state.lock().unwrap();
            (state.reader.take(), state.writer.take())
        };
        if let Some(w) = reader {
            w.wake();
        }
        if let Some(w) = writer {
            w.wake();
        }
    }
    fn abort(&self, error: Error) {
        let mut state = self.state.lock().unwrap();
        if state.end.is_none() {
            state.end = Some(Err(error));
        }
        drop(state);
        self.wake();
    }
    fn disconnect(&self) {
        let mut state = self.state.lock().unwrap();
        state.consumer_alive = false;
        state.chunk = None;
        drop(state);
        self.wake();
    }
    fn complete(&self, mut trailers: Option<FieldMap>) -> Result<(), Error> {
        let mut state = self.state.lock().unwrap();
        if let Some(end) = &state.end {
            return match end {
                Err(e) => Err(e.clone()),
                Ok(_) => Err(Error::HttpProtocolError),
            };
        }
        let result = if state.expected.is_some_and(|n| n != state.written) {
            Err(state.context.size_error(Some(state.written)))
        } else {
            if let Some(fields) = &mut trailers {
                fields.set_immutable();
            }
            Ok(trailers)
        };
        let returned = result.as_ref().map(|_| ()).map_err(Clone::clone);
        state.end = Some(result);
        drop(state);
        self.wake();
        returned
    }
}
fn register(slot: &mut Option<Waker>, cx: &Context<'_>) {
    if !slot.as_ref().is_some_and(|old| old.will_wake(cx.waker())) {
        *slot = Some(cx.waker().clone());
    }
}
fn next_size(state: &State, len: usize) -> Result<u64, Error> {
    let n = state
        .written
        .checked_add(len as u64)
        .ok_or_else(|| state.context.size_error(None))?;
    if state.expected.is_some_and(|expected| n > expected) {
        Err(state.context.size_error(Some(n)))
    } else {
        Ok(n)
    }
}
fn stream_error(state: &State, reported: &mut bool) -> Option<StreamError> {
    if let Some(Err(error)) = &state.end {
        if !*reported {
            *reported = true;
            return Some(StreamError::LastOperationFailed(wasmtime::Error::new(
                error.clone(),
            )));
        }
        return Some(StreamError::Closed);
    }
    if !state.consumer_alive || state.end.is_some() {
        Some(StreamError::Closed)
    } else {
        None
    }
}

/// Owns completion independently of its child output stream. Dropping the child
/// cannot accidentally turn an unfinished request/response into a successful EOF.
pub struct OutgoingBody {
    shared: Option<Arc<Shared>>,
    stream: Option<Box<dyn OutputStream>>,
}
impl OutgoingBody {
    pub fn new(context: StreamContext, expected: Option<u64>) -> (Self, BodyReceiver) {
        let shared = Shared::new(context, expected);
        let stream = Box::new(BodyWriteStream {
            shared: shared.clone(),
            permit: 0,
            reported: false,
        });
        (
            Self {
                shared: Some(shared.clone()),
                stream: Some(stream),
            },
            BodyReceiver {
                shared,
                ended: false,
                #[cfg(feature = "p3")]
                frames: None,
            },
        )
    }
    pub fn take_stream(&mut self) -> Option<Box<dyn OutputStream>> {
        self.stream.take()
    }
    /// The WIT resource table also enforces this child lifetime before deletion.
    pub fn finish(mut self, trailers: Option<FieldMap>) -> wasmtime::Result<Result<(), Error>> {
        drop(self.stream.take());
        let shared = self.shared.as_ref().unwrap();
        wasmtime::ensure!(
            !shared.state.lock().unwrap().stream_alive,
            "HTTP output stream must be dropped before finishing its body"
        );
        let result = shared.complete(trailers);
        self.shared.take();
        Ok(result)
    }
}
impl Drop for OutgoingBody {
    fn drop(&mut self) {
        if let Some(shared) = &self.shared {
            shared.abort(Error::HttpProtocolError);
        }
    }
}

/// Native transport consumes at most one bounded chunk before asking for more.
/// Removing a frame releases queue capacity, not a promise of peer receipt.
pub enum BodyFrame {
    Data(Bytes),
    Trailers(FieldMap),
}
pub struct BodyReceiver {
    shared: Arc<Shared>,
    ended: bool,
    #[cfg(feature = "p3")]
    frames: Option<futures_util::stream::BoxStream<'static, Result<BodyFrame, Error>>>,
}
impl BodyReceiver {
    #[cfg(feature = "p3")]
    pub(crate) fn from_frames(
        frames: futures_util::stream::BoxStream<'static, Result<BodyFrame, Error>>,
    ) -> Self {
        Self {
            shared: Shared::new(StreamContext::Request, None),
            ended: false,
            frames: Some(frames),
        }
    }
    pub async fn next(&mut self) -> Result<Option<BodyFrame>, Error> {
        #[cfg(feature = "p3")]
        if let Some(frames) = &mut self.frames {
            use futures_util::StreamExt;
            return frames.next().await.transpose();
        }
        futures_lite::future::poll_fn(|cx| {
            if self.ended {
                return Poll::Ready(Ok(None));
            }
            let mut state = self.shared.state.lock().unwrap();
            if let Some(bytes) = state.chunk.take() {
                drop(state);
                self.shared.wake();
                return Poll::Ready(Ok(Some(BodyFrame::Data(bytes))));
            }
            if let Some(end) = &state.end {
                self.ended = true;
                return Poll::Ready(end.clone().map(|fields| fields.map(BodyFrame::Trailers)));
            }
            register(&mut state.reader, cx);
            Poll::Pending
        })
        .await
    }
    pub fn fail(&mut self, error: Error) {
        #[cfg(feature = "p3")]
        {
            self.frames = None;
        }
        self.shared.abort(error);
        self.shared.disconnect();
        self.ended = true;
    }
}
impl Drop for BodyReceiver {
    fn drop(&mut self) {
        self.shared.disconnect();
    }
}

struct BodyWriteStream {
    shared: Arc<Shared>,
    permit: usize,
    reported: bool,
}
#[async_trait::async_trait]
impl OutputStream for BodyWriteStream {
    fn check_write(&mut self) -> Result<usize, StreamError> {
        let state = self.shared.state.lock().unwrap();
        if let Some(error) = stream_error(&state, &mut self.reported) {
            return Err(error);
        }
        self.permit = if state.chunk.is_none() { CHUNK_SIZE } else { 0 };
        Ok(self.permit)
    }
    fn write(&mut self, bytes: Bytes) -> Result<(), StreamError> {
        let mut state = self.shared.state.lock().unwrap();
        if let Some(error) = stream_error(&state, &mut self.reported) {
            return Err(error);
        }
        if bytes.len() > self.permit || (state.chunk.is_some() && !bytes.is_empty()) {
            return Err(StreamError::Trap(wasmtime::format_err!(
                "HTTP write exceeded its permit"
            )));
        }
        self.permit = 0;
        let n = match next_size(&state, bytes.len()) {
            Ok(n) => n,
            Err(error) => {
                state.end = Some(Err(error.clone()));
                self.reported = true;
                drop(state);
                self.shared.wake();
                return Err(StreamError::LastOperationFailed(wasmtime::Error::new(
                    error,
                )));
            }
        };
        state.written = n;
        if !bytes.is_empty() {
            state.chunk = Some(bytes);
        }
        drop(state);
        self.shared.wake();
        Ok(())
    }
    fn flush(&mut self) -> Result<(), StreamError> {
        // Bytes have already been handed to the bounded HTTP queue. Readiness
        // applies backpressure until the transport consumes its queued frame.
        let state = self.shared.state.lock().unwrap();
        match stream_error(&state, &mut self.reported) {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}
#[async_trait::async_trait]
impl Pollable for BodyWriteStream {
    async fn ready(&mut self) {
        futures_lite::future::poll_fn(|cx| {
            let mut state = self.shared.state.lock().unwrap();
            if state.chunk.is_none() || state.end.is_some() || !state.consumer_alive {
                Poll::Ready(())
            } else {
                register(&mut state.writer, cx);
                Poll::Pending
            }
        })
        .await
    }
}
impl Drop for BodyWriteStream {
    fn drop(&mut self) {
        self.shared.state.lock().unwrap().stream_alive = false;
        self.shared.wake();
    }
}
