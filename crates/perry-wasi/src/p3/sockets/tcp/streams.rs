//! TCP streams retain owned native requests and implement half-close on drop.
use super::{ErrorCode, Native, Slot, SocketResult, TcpSocket};
use crate::sockets::{
    tcp::{Failure, TcpConnection},
    WasiSocketsCtxView,
};
use bytes::Bytes;
use futures_channel::oneshot;
use futures_util::future::BoxFuture;
use perry_component_io::ResourceRequest;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use wasmtime::{
    component::{
        Destination, Resource, Source, StreamConsumer, StreamProducer, StreamResult, VecBuffer,
    },
    StoreContextMut,
};

type ReadTask = ResourceRequest<Result<Vec<u8>, Failure>>;
type UnitTask = ResourceRequest<Result<(), Failure>>;
type Outcome = Result<(), ErrorCode>;
fn complete<T>(result: Result<Result<T, Failure>, perry_component_io::Error>) -> SocketResult<T> {
    Ok(result??)
}
pub(super) struct Listener<T> {
    native: Slot<Native>,
    pending: Option<BoxFuture<'static, SocketResult<Native>>>,
    getter: for<'a> fn(&'a mut T) -> WasiSocketsCtxView<'a>,
}
impl<T> Listener<T> {
    pub fn new(
        native: Slot<Native>,
        getter: for<'a> fn(&'a mut T) -> WasiSocketsCtxView<'a>,
    ) -> Self {
        Self {
            native,
            pending: None,
            getter,
        }
    }
}
impl<T: 'static> StreamProducer<T> for Listener<T> {
    type Item = Resource<TcpSocket>;
    type Buffer = Option<Self::Item>;
    fn poll_produce<'a>(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut store: StoreContextMut<'a, T>,
        mut dst: Destination<'a, Self::Item, Self::Buffer>,
        finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        if dst.remaining(&mut store) == Some(0) {
            return Poll::Ready(Ok(StreamResult::Completed));
        }
        if self.pending.is_none() {
            let accept = self.native.with(|s| Ok(s.accept()))?;
            self.pending = Some(Box::pin(async move { Ok(accept.await?) }));
        }
        let result = match self.pending.as_mut().unwrap().as_mut().poll(cx) {
            Poll::Pending if finish => return Poll::Ready(Ok(StreamResult::Cancelled)),
            Poll::Pending => return Poll::Pending,
            Poll::Ready(result) => result,
        };
        self.pending = None;
        match result {
            Ok(native) => {
                let socket = (self.getter)(store.data_mut())
                    .table
                    .push(TcpSocket::new(native))?;
                dst.set_buffer(Some(socket));
                Poll::Ready(Ok(StreamResult::Completed))
            }
            Err(error) => match error.downcast()? {
                ErrorCode::ConnectionAborted
                | ErrorCode::ConnectionReset
                | ErrorCode::RemoteUnreachable
                | ErrorCode::NotSupported => {
                    // Accept can surface a transient error for a peer that has
                    // already disappeared. Keep the listener stream alive.
                    cx.waker().wake_by_ref();
                    if finish {
                        Poll::Ready(Ok(StreamResult::Cancelled))
                    } else {
                        Poll::Pending
                    }
                }
                _ => Poll::Ready(Ok(StreamResult::Dropped)),
            },
        }
    }
}
pub(super) struct Input {
    connection: TcpConnection,
    pending: Option<ReadTask>,
    bytes: Bytes,
    outcome: Option<oneshot::Sender<Outcome>>,
}
impl Input {
    pub fn new(connection: TcpConnection, outcome: oneshot::Sender<Outcome>) -> Self {
        Self {
            connection,
            pending: None,
            bytes: Bytes::new(),
            outcome: Some(outcome),
        }
    }
    fn close(&mut self, outcome: Outcome) -> Poll<wasmtime::Result<StreamResult>> {
        if let Some(send) = self.outcome.take() {
            let _ = send.send(outcome);
        }
        Poll::Ready(Ok(StreamResult::Dropped))
    }
}
impl Drop for Input {
    fn drop(&mut self) {
        // The driver's cleanup task retains native inputs even if neither end
        // of the component stream nor its result future is retained.
        let _ = self.connection.close_receive(self.pending.take());
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
        if dst.remaining(&mut store) == Some(0) {
            return Poll::Ready(Ok(StreamResult::Completed));
        }
        if self.outcome.is_none() {
            return Poll::Ready(Ok(StreamResult::Dropped));
        }
        if self.bytes.is_empty() {
            if self.pending.is_none() {
                self.pending = Some(self.connection.read(crate::MAX_READ_SIZE_ALLOC)?);
            }
            let result = match Pin::new(self.pending.as_mut().unwrap()).poll(cx) {
                Poll::Pending if finish => return Poll::Ready(Ok(StreamResult::Cancelled)),
                Poll::Pending => return Poll::Pending,
                Poll::Ready(result) => complete(result),
            };
            self.pending = None;
            match result {
                Ok(bytes) if bytes.is_empty() => return self.close(Ok(())),
                Ok(bytes) => self.bytes = bytes.into(),
                Err(error) => return self.close(Err(error.downcast()?)),
            }
        }
        let mut dst = dst.as_direct(store, crate::MAX_READ_SIZE_ALLOC);
        let count = dst.remaining().len().min(self.bytes.len());
        dst.remaining()[..count].copy_from_slice(&self.bytes.split_to(count));
        dst.mark_written(count);
        Poll::Ready(Ok(StreamResult::Completed))
    }
}
pub(super) struct ClosedOutput {
    error: Option<ErrorCode>,
    shutdown: Result<UnitTask, perry_component_io::Error>,
}
pub(super) struct Output {
    connection: TcpConnection,
    pending: Option<UnitTask>,
    error: Option<ErrorCode>,
    outcome: Option<oneshot::Sender<ClosedOutput>>,
}
impl Output {
    pub fn new(connection: TcpConnection, outcome: oneshot::Sender<ClosedOutput>) -> Self {
        Self {
            connection,
            pending: None,
            error: None,
            outcome: Some(outcome),
        }
    }
    fn poll_write(&mut self, cx: &mut Context<'_>) -> Poll<wasmtime::Result<StreamResult>> {
        let result = std::task::ready!(Pin::new(self.pending.as_mut().unwrap()).poll(cx));
        self.pending = None;
        match complete(result) {
            Ok(()) => Poll::Ready(Ok(StreamResult::Completed)),
            Err(error) => {
                self.error = Some(error.downcast()?);
                Poll::Ready(Ok(StreamResult::Dropped))
            }
        }
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        let shutdown = self.connection.close_send(self.pending.take());
        if let Some(send) = self.outcome.take() {
            let _ = send.send(ClosedOutput {
                error: self.error.take(),
                shutdown,
            });
        }
    }
}
impl<T> StreamConsumer<T> for Output {
    type Item = u8;
    fn poll_consume(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        store: StoreContextMut<T>,
        src: Source<u8>,
        _: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        // Once bytes have been accepted, finish/cancellation must await that
        // write. Retrying accepted bytes would duplicate data on the wire.
        if self.pending.is_some() {
            return self.poll_write(cx);
        }
        let mut src = src.as_direct(store);
        let count = src.remaining().len().min(crate::MAX_READ_SIZE_ALLOC);
        if count == 0 {
            return Poll::Ready(Ok(StreamResult::Completed));
        }
        let bytes = src.remaining()[..count].to_vec();
        self.pending = Some(self.connection.write(bytes)?);
        src.mark_read(count);
        self.poll_write(cx)
    }
}
pub(super) async fn finish_output(
    receive: oneshot::Receiver<ClosedOutput>,
) -> wasmtime::Result<Outcome> {
    let closed = receive
        .await
        .map_err(|_| wasmtime::format_err!("TCP output completion was lost"))?;
    let shutdown = complete(closed.shutdown?.await);
    match (closed.error, shutdown) {
        (Some(error), Ok(())) => Ok(Err(error)),
        (None, Ok(())) => Ok(Ok(())),
        (error, Err(shutdown)) => {
            let shutdown = shutdown.downcast()?;
            Ok(Err(error.unwrap_or(shutdown)))
        }
    }
}
