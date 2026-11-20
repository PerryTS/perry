//! Owned P2 stream views adapt native Turnloop I/O to the P3 stream ABI.
//! Readiness futures retain native views only, never a Store or guest memory.

use bytes::Bytes;
use futures_channel::oneshot;
use futures_util::future::BoxFuture;
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use wasmtime::component::{
    Destination, Source, StreamConsumer, StreamProducer, StreamResult, VecBuffer,
};
use wasmtime::{AsContextMut, StoreContextMut};
use wasmtime_wasi_io::streams::{InputStream, OutputStream, StreamError};

pub(super) trait StreamFailure: Send + Unpin + 'static {
    fn from_io(error: &std::io::Error) -> Self;
    fn closed() -> Self;
}
type Outcome<E> = oneshot::Sender<Result<(), E>>;

pub(super) struct Input<E> {
    stream: Option<Box<dyn InputStream>>,
    ready: Option<BoxFuture<'static, Box<dyn InputStream>>>,
    outcome: Option<Outcome<E>>,
}
impl<E> Input<E> {
    pub fn new(stream: Box<dyn InputStream>, outcome: Outcome<E>) -> Self {
        Self {
            stream: Some(stream),
            ready: None,
            outcome: Some(outcome),
        }
    }
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if self.ready.is_none() {
            let mut stream = self.stream.take().unwrap();
            self.ready = Some(Box::pin(async move {
                stream.ready().await;
                stream
            }));
        }
        let stream = std::task::ready!(self.ready.as_mut().unwrap().as_mut().poll(cx));
        self.ready = None;
        self.stream = Some(stream);
        Poll::Ready(())
    }
}

impl<D, E: StreamFailure> StreamProducer<D> for Input<E> {
    type Item = u8;
    type Buffer = VecBuffer<u8>;
    fn poll_produce<'a>(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut store: StoreContextMut<'a, D>,
        dst: Destination<'a, u8, VecBuffer<u8>>,
        finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        // The stream ABI permits a readiness-only probe to return immediately.
        if dst.remaining(store.as_context_mut()) == Some(0) {
            return Poll::Ready(Ok(StreamResult::Completed));
        }
        if self.poll_ready(cx).is_pending() {
            return if finish {
                Poll::Ready(Ok(StreamResult::Cancelled))
            } else {
                Poll::Pending
            };
        }
        let mut dst = dst.as_direct(store, crate::MAX_READ_SIZE_ALLOC);
        let capacity = dst.remaining().len().min(crate::MAX_READ_SIZE_ALLOC);
        match self.stream.as_mut().unwrap().read(capacity) {
            Ok(bytes) if bytes.is_empty() => {
                // Another stdin view may have consumed the shared ready bytes.
                if finish {
                    Poll::Ready(Ok(StreamResult::Cancelled))
                } else {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            }
            Ok(bytes) => {
                if bytes.len() > capacity {
                    return Poll::Ready(Err(wasmtime::format_err!(
                        "input stream exceeded its read capacity"
                    )));
                }
                dst.remaining()[..bytes.len()].copy_from_slice(&bytes);
                dst.mark_written(bytes.len());
                Poll::Ready(Ok(StreamResult::Completed))
            }
            Err(StreamError::Closed) => {
                if let Some(outcome) = self.outcome.take() {
                    let _ = outcome.send(Ok(()));
                }
                Poll::Ready(Ok(StreamResult::Dropped))
            }
            Err(error) => dropped(&mut self.outcome, error),
        }
    }
}

pub(super) struct Output<E> {
    stream: Option<Box<dyn OutputStream>>,
    ready: Option<BoxFuture<'static, Box<dyn OutputStream>>>,
    outcome: Option<Outcome<E>>,
    flushing: bool,
}
impl<E: StreamFailure> Output<E> {
    pub fn new(stream: Box<dyn OutputStream>, outcome: Outcome<E>) -> Self {
        Self {
            stream: Some(stream),
            ready: None,
            outcome: Some(outcome),
            flushing: false,
        }
    }
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if self.ready.is_none() {
            let mut stream = self.stream.take().unwrap();
            self.ready = Some(Box::pin(async move {
                stream.ready().await;
                stream
            }));
        }
        let stream = std::task::ready!(self.ready.as_mut().unwrap().as_mut().poll(cx));
        self.ready = None;
        self.stream = Some(stream);
        Poll::Ready(())
    }
    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<wasmtime::Result<StreamResult>> {
        // Bytes are already accepted. Even for finish=true, preserve flush
        // backpressure: reporting cancellation could hide partial native writes.
        std::task::ready!(self.poll_ready(cx));
        match self.stream.as_mut().unwrap().flush() {
            Ok(()) => {
                self.flushing = false;
                Poll::Ready(Ok(StreamResult::Completed))
            }
            Err(error) => dropped(&mut self.outcome, error),
        }
    }
}
impl<D, E: StreamFailure> StreamConsumer<D> for Output<E> {
    type Item = u8;
    fn poll_consume(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        store: StoreContextMut<D>,
        src: Source<u8>,
        finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        if self.flushing {
            return self.poll_flush(cx);
        }
        if self.poll_ready(cx).is_pending() {
            return if finish {
                Poll::Ready(Ok(StreamResult::Cancelled))
            } else {
                Poll::Pending
            };
        }
        let permit = match self.stream.as_mut().unwrap().check_write() {
            Ok(0) => {
                return if finish {
                    Poll::Ready(Ok(StreamResult::Cancelled))
                } else {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                };
            }
            Ok(permit) => permit.min(crate::MAX_READ_SIZE_ALLOC),
            Err(error) => return dropped(&mut self.outcome, error),
        };
        {
            let mut src = src.as_direct(store);
            let length = src.remaining().len().min(permit);
            if length == 0 {
                return Poll::Ready(Ok(StreamResult::Completed));
            }
            let bytes = Bytes::copy_from_slice(&src.remaining()[..length]);
            if let Err(error) = self.stream.as_mut().unwrap().write(bytes) {
                return dropped(&mut self.outcome, error);
            }
            src.mark_read(length);
        }
        if let Err(error) = self.stream.as_mut().unwrap().flush() {
            return dropped(&mut self.outcome, error);
        }
        self.flushing = true;
        self.poll_flush(cx)
    }
}

fn dropped<E: StreamFailure>(
    outcome: &mut Option<Outcome<E>>,
    error: StreamError,
) -> Poll<wasmtime::Result<StreamResult>> {
    let code = match error {
        StreamError::Trap(error) => return Poll::Ready(Err(error)),
        StreamError::Closed => E::closed(),
        StreamError::LastOperationFailed(error) => match error.downcast_ref::<std::io::Error>() {
            Some(error) => E::from_io(error),
            None => E::from_io(&std::io::Error::other(error.to_string())),
        },
    };
    if let Some(outcome) = outcome.take() {
        let _ = outcome.send(Err(code));
    }
    Poll::Ready(Ok(StreamResult::Dropped))
}
