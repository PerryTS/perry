use super::*;
use wasmtime_wasi_io::streams::InputStream;

pub struct IncomingBody {
    // Kept only by guest-facing owners, never by the native sender: no task cycle.
    transport: Option<Arc<perry_component_io::Request<()>>>,
    shared: Option<Arc<Shared>>,
    stream: Option<Box<dyn InputStream>>,
}
pub struct IncomingBodySender {
    shared: Arc<Shared>,
    finished: bool,
}
pub struct FutureTrailers {
    _transport: Option<Arc<perry_component_io::Request<()>>>,
    shared: Arc<Shared>,
    taken: bool,
}

impl IncomingBody {
    #[cfg(feature = "p3")]
    pub(crate) fn abort_handle(&self) -> impl FnOnce(Error) + Send + 'static {
        let shared = self.shared.as_ref().unwrap().clone();
        move |error| shared.abort(error)
    }
    pub(crate) fn retain_transport(&mut self, task: Arc<perry_component_io::Request<()>>) {
        self.transport = Some(task);
    }
    pub fn new(context: StreamContext, expected: Option<u64>) -> (Self, IncomingBodySender) {
        let shared = Shared::new(context, expected);
        let stream = Box::new(BodyReadStream {
            shared: shared.clone(),
            reported: false,
        });
        (
            Self {
                transport: None,
                shared: Some(shared.clone()),
                stream: Some(stream),
            },
            IncomingBodySender {
                shared,
                finished: false,
            },
        )
    }
    pub fn take_stream(&mut self) -> Option<Box<dyn InputStream>> {
        self.stream.take()
    }
    /// Stop delivering data and drain the remaining protocol body to its trailers.
    /// The native producer continues, while all newly delivered chunks are discarded.
    pub fn finish(mut self) -> wasmtime::Result<FutureTrailers> {
        drop(self.stream.take());
        let shared = self.shared.as_ref().unwrap();
        let mut state = shared.state.lock().unwrap();
        wasmtime::ensure!(
            !state.stream_alive,
            "HTTP input stream must be dropped before finishing its body"
        );
        state.discard = true;
        state.chunk = None;
        drop(state);
        shared.wake();
        Ok(FutureTrailers {
            _transport: self.transport.take(),
            shared: self.shared.take().unwrap(),
            taken: false,
        })
    }
}
impl Drop for IncomingBody {
    fn drop(&mut self) {
        if let Some(task) = &self.transport {
            task.cancel();
        }
        if let Some(shared) = &self.shared {
            shared.disconnect();
        }
    }
}

impl IncomingBodySender {
    pub(crate) fn consumer_closed(&self) -> bool {
        !self.shared.state.lock().unwrap().consumer_alive
    }
    /// A cancelled pending send has accepted no bytes. The protocol driver must
    /// retain native buffers separately until any native I/O cancellation completes.
    pub async fn push(&mut self, bytes: Bytes) -> Result<(), Error> {
        if bytes.len() > CHUNK_SIZE {
            return Err(Error::InternalError(Some(
                "HTTP input chunk exceeds bounded capacity".into(),
            )));
        }
        let mut bytes = Some(bytes);
        futures_lite::future::poll_fn(|cx| {
            let mut state = self.shared.state.lock().unwrap();
            if let Some(Err(e)) = &state.end {
                return Poll::Ready(Err(e.clone()));
            }
            if !state.consumer_alive || state.end.is_some() {
                return Poll::Ready(Err(Error::ConnectionTerminated));
            }
            if bytes.as_ref().unwrap().is_empty() {
                return Poll::Ready(Ok(()));
            }
            if !state.discard && state.chunk.is_some() {
                register(&mut state.writer, cx);
                return Poll::Pending;
            }
            let data = bytes.take().unwrap();
            let next = match next_size(&state, data.len()) {
                Ok(n) => n,
                Err(e) => {
                    state.end = Some(Err(e.clone()));
                    drop(state);
                    self.shared.wake();
                    return Poll::Ready(Err(e));
                }
            };
            state.written = next;
            if !state.discard && !data.is_empty() {
                state.chunk = Some(data);
            }
            drop(state);
            self.shared.wake();
            Poll::Ready(Ok(()))
        })
        .await
    }
    pub fn finish(mut self, trailers: Option<FieldMap>) -> Result<(), Error> {
        let result = self.shared.complete(trailers);
        self.finished = true;
        result
    }
    pub fn fail(mut self, error: Error) {
        self.shared.abort(error);
        self.finished = true;
    }
}
impl Drop for IncomingBodySender {
    fn drop(&mut self) {
        if !self.finished {
            let error = match self.shared.state.lock().unwrap().context {
                StreamContext::Request => Error::HttpProtocolError,
                StreamContext::Response => Error::HttpResponseIncomplete,
            };
            self.shared.abort(error);
        }
    }
}
impl FutureTrailers {
    /// None means pending; the outer error marks an already consumed result.
    pub fn get(&mut self) -> Option<Result<Completion, ()>> {
        if self.taken {
            return Some(Err(()));
        }
        let state = self.shared.state.lock().unwrap();
        let end = state.end.clone()?;
        self.taken = true;
        Some(Ok(end))
    }
    pub async fn ready(&mut self) {
        futures_lite::future::poll_fn(|cx| {
            let mut state = self.shared.state.lock().unwrap();
            if self.taken || state.end.is_some() {
                Poll::Ready(())
            } else {
                register(&mut state.reader, cx);
                Poll::Pending
            }
        })
        .await
    }
}
impl Drop for FutureTrailers {
    fn drop(&mut self) {
        if let Some(task) = &self._transport {
            task.cancel();
        }
        self.shared.disconnect();
    }
}

struct BodyReadStream {
    shared: Arc<Shared>,
    reported: bool,
}
#[async_trait::async_trait]
impl InputStream for BodyReadStream {
    fn read(&mut self, size: usize) -> Result<Bytes, StreamError> {
        let mut state = self.shared.state.lock().unwrap();
        if let Some(bytes) = &mut state.chunk {
            let result = bytes.split_to(bytes.len().min(size));
            if bytes.is_empty() {
                state.chunk = None;
            }
            drop(state);
            self.shared.wake();
            return Ok(result);
        }
        match stream_error(&state, &mut self.reported) {
            Some(e) => Err(e),
            None => Ok(Bytes::new()),
        }
    }
}
#[async_trait::async_trait]
impl Pollable for BodyReadStream {
    async fn ready(&mut self) {
        futures_lite::future::poll_fn(|cx| {
            let mut state = self.shared.state.lock().unwrap();
            if state.chunk.is_some() || state.end.is_some() || !state.consumer_alive {
                Poll::Ready(())
            } else {
                register(&mut state.reader, cx);
                Poll::Pending
            }
        })
        .await
    }
}
impl Drop for BodyReadStream {
    fn drop(&mut self) {
        self.shared.state.lock().unwrap().stream_alive = false;
        self.shared.wake();
    }
}
