use super::*;
use crate::http::body::FutureTrailers;
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use wasmtime::{
    component::{Destination, StreamProducer, StreamResult, VecBuffer},
    StoreContextMut,
};
use wasmtime_wasi_io::streams::{InputStream, StreamError};

type End = Result<Option<FutureTrailers>, ErrorCode>;
struct Input {
    body: Option<IncomingBody>,
    stream: Option<Box<dyn InputStream>>,
    ready: Option<BoxFuture<'static, Box<dyn InputStream>>>,
    end: Option<oneshot::Sender<End>>,
}
impl Input {
    fn complete(&mut self, result: Result<(), ErrorCode>) -> wasmtime::Result<()> {
        self.ready = None;
        self.stream = None;
        let body = self.body.take().unwrap();
        let result = match result {
            Ok(()) => Ok(Some(body.finish()?)),
            Err(error) => Err(error),
        };
        if let Some(end) = self.end.take() {
            let _ = end.send(result);
        }
        Ok(())
    }
}
impl Drop for Input {
    fn drop(&mut self) {
        // Dropping the readable endpoint cancels delivery; it is not a claim
        // that the remaining body or its trailers were successfully received.
        if let Some(end) = self.end.take() {
            let _ = end.send(Ok(None));
        }
    }
}
impl<T> StreamProducer<T> for Input {
    type Item = u8;
    type Buffer = VecBuffer<u8>;
    fn poll_produce<'a>(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut store: StoreContextMut<'a, T>,
        dst: Destination<'a, u8, VecBuffer<u8>>,
        finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        if self.end.is_none() {
            return Poll::Ready(Ok(StreamResult::Dropped));
        }
        if dst.remaining(&mut store) == Some(0) {
            return Poll::Ready(Ok(StreamResult::Completed));
        }
        if self.ready.is_none() {
            let mut stream = self.stream.take().unwrap();
            self.ready = Some(Box::pin(async move {
                stream.ready().await;
                stream
            }));
        }
        let stream = match self.ready.as_mut().unwrap().as_mut().poll(cx) {
            Poll::Ready(stream) => stream,
            Poll::Pending if finish => return Poll::Ready(Ok(StreamResult::Cancelled)),
            Poll::Pending => return Poll::Pending,
        };
        self.ready = None;
        self.stream = Some(stream);
        let mut dst = dst.as_direct(store, crate::http::body::CHUNK_SIZE);
        let capacity = dst.remaining().len();
        match self.stream.as_mut().unwrap().read(capacity) {
            Ok(bytes) if bytes.is_empty() => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Ok(bytes) => {
                if bytes.len() > capacity {
                    return Poll::Ready(Err(wasmtime::format_err!(
                        "HTTP input exceeded destination capacity"
                    )));
                }
                dst.remaining()[..bytes.len()].copy_from_slice(&bytes);
                dst.mark_written(bytes.len());
                Poll::Ready(Ok(StreamResult::Completed))
            }
            Err(StreamError::Closed) => {
                self.complete(Ok(()))?;
                Poll::Ready(Ok(StreamResult::Dropped))
            }
            Err(StreamError::LastOperationFailed(error)) => {
                let error = error.downcast::<Error>()?;
                self.complete(Err(error.into()))?;
                Poll::Ready(Ok(StreamResult::Dropped))
            }
            Err(StreamError::Trap(error)) => Poll::Ready(Err(error)),
        }
    }
}
pub(super) fn consume<T: 'static>(
    store: &mut Access<'_, T, WasiHttp>,
    mut body: IncomingBody,
    getter: fn(&mut T) -> WasiHttpCtxView<'_>,
) -> wasmtime::Result<(StreamReader<u8>, FutureReader<TrailerResult>)> {
    let (end, receive) = oneshot::channel::<End>();
    let stream = body
        .take_stream()
        .ok_or_else(|| wasmtime::format_err!("HTTP body stream was already consumed"))?;
    let contents = StreamReader::new(
        &mut *store,
        Input {
            body: Some(body),
            stream: Some(stream),
            ready: None,
            end: Some(end),
        },
    )?;
    let trailers = helpers::future(
        &mut *store,
        async move {
            let result = match receive.await {
                Ok(Ok(Some(mut trailers))) => {
                    trailers.ready().await;
                    trailers
                        .get()
                        .ok_or_else(|| wasmtime::format_err!("HTTP trailers remained pending"))?
                        .map_err(|_| wasmtime::format_err!("HTTP trailers consumed twice"))?
                        .map_err(ErrorCode::from)
                }
                Ok(Ok(None)) => Ok(None),
                Ok(Err(error)) => Err(error),
                Err(_) => return Err(wasmtime::format_err!("HTTP body completion was lost")),
            };
            wasmtime::error::Ok(result)
        },
        move |data, result| {
            Ok(match result {
                Ok(Some(fields)) => Ok(Some(getter(data).table.push(fields)?)),
                Ok(None) => Ok(None),
                Err(error) => Err(error),
            })
        },
    )?;
    Ok((contents, trailers))
}
