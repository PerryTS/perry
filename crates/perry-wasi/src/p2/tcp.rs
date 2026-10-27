//! WASI 0.2 TCP start/finish and pollable bookkeeping over owned Turnloop work.
use crate::{
    io::poll::Pollable,
    p2::{bindings::sockets::network::ErrorCode, SocketError, SocketResult},
    sockets::tcp::{Family, TcpSocket as NativeSocket},
};
use futures_util::{future::BoxFuture, FutureExt};
use std::{
    net::SocketAddr,
    sync::Arc,
    task::{Context, Poll},
};
use wasmtime_wasi_io::streams::{DynInputStream, DynOutputStream};
mod streams;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AsyncOperation {
    Bind,
    Connect,
    Listen,
}

pub struct TcpSocket {
    pub(crate) inner: Option<NativeSocket>,
    pub(crate) operation: Option<AsyncOperation>,
    pub(crate) family: Family,
    pub(crate) network: Arc<()>,
    connecting: Option<BoxFuture<'static, (NativeSocket, SocketResult<()>)>>,
    connection_result: Option<SocketResult<()>>,
    accepting: Option<BoxFuture<'static, SocketResult<NativeSocket>>>,
    accepted: Option<SocketResult<NativeSocket>>,
    streams: Option<streams::Streams>,
}
impl TcpSocket {
    pub(crate) fn new(inner: NativeSocket, network: Arc<()>) -> Self {
        Self {
            family: inner.family(),
            inner: Some(inner),
            operation: None,
            network,
            connecting: None,
            connection_result: None,
            accepting: None,
            accepted: None,
            streams: None,
        }
    }
    pub(crate) fn socket(&self) -> SocketResult<&NativeSocket> {
        self.inner.as_ref().ok_or_else(|| {
            if self.connecting.is_some() {
                ErrorCode::ConcurrencyConflict.into()
            } else {
                ErrorCode::InvalidState.into()
            }
        })
    }
    pub(crate) fn socket_mut(&mut self) -> SocketResult<&mut NativeSocket> {
        self.inner.as_mut().ok_or_else(|| {
            if self.connecting.is_some() {
                ErrorCode::ConcurrencyConflict.into()
            } else {
                ErrorCode::InvalidState.into()
            }
        })
    }
    pub(crate) fn idle(&self) -> SocketResult<()> {
        if self.operation.is_some() {
            return Err(ErrorCode::ConcurrencyConflict.into());
        }
        self.socket()?;
        Ok(())
    }
    pub(crate) fn check_network(&self, network: &Arc<()>) -> SocketResult<()> {
        if Arc::ptr_eq(&self.network, network) {
            Ok(())
        } else {
            Err(ErrorCode::InvalidArgument.into())
        }
    }
    pub(crate) fn start_connect(&mut self, address: SocketAddr) -> SocketResult<()> {
        self.idle()?;
        self.socket()?.check_connect(address)?;
        let mut socket = self.inner.take().expect("validated native socket");
        self.connecting = Some(
            async move {
                let result = socket.connect(address).await;
                (socket, result)
            }
            .boxed(),
        );
        self.operation = Some(AsyncOperation::Connect);
        Ok(())
    }
    fn poll_connect(&mut self, cx: &mut Context<'_>) -> Poll<wasmtime::Result<()>> {
        if let Some(task) = &mut self.connecting {
            let (socket, result) = std::task::ready!(task.as_mut().poll(cx));
            self.connecting = None;
            // A failed attempt permanently closes the socket, including failed
            // permission checks that ran inside its asynchronous connection.
            if result.is_ok() {
                self.inner = Some(socket);
            }
            self.connection_result = Some(result);
        }
        Poll::Ready(readiness_error(self.connection_result.as_ref()))
    }
    pub(crate) fn finish_connect(&mut self) -> SocketResult<(DynInputStream, DynOutputStream)> {
        if self.operation != Some(AsyncOperation::Connect) {
            return Err(ErrorCode::NotInProgress.into());
        }
        match self.poll_connect(&mut noop()) {
            Poll::Pending => return Err(ErrorCode::WouldBlock.into()),
            Poll::Ready(Err(error)) => return Err(SocketError::trap(error)),
            Poll::Ready(Ok(())) => (),
        }
        self.operation = None;
        self.connection_result
            .take()
            .expect("completed connection result")?;
        self.take_streams()
    }
    fn poll_accept(&mut self, cx: &mut Context<'_>) -> Poll<wasmtime::Result<()>> {
        if self.accepted.is_none() {
            if self.accepting.is_none() {
                let socket = match self.socket() {
                    Ok(socket) => socket,
                    Err(error) => return Poll::Ready(Err(error.into())),
                };
                self.accepting = Some(socket.accept().boxed());
            }
            let result = std::task::ready!(self
                .accepting
                .as_mut()
                .expect("accept future")
                .as_mut()
                .poll(cx));
            self.accepting = None;
            self.accepted = Some(result);
        }
        Poll::Ready(readiness_error(self.accepted.as_ref()))
    }
    pub(crate) fn accept(&mut self) -> SocketResult<Self> {
        self.idle()?;
        if !self.socket()?.is_listening() {
            return Err(ErrorCode::InvalidState.into());
        }
        match self.poll_accept(&mut noop()) {
            Poll::Pending => return Err(ErrorCode::WouldBlock.into()),
            Poll::Ready(Err(error)) => return Err(SocketError::trap(error)),
            Poll::Ready(Ok(())) => (),
        }
        Ok(Self::new(
            self.accepted.take().expect("accepted result")?,
            self.network.clone(),
        ))
    }
    pub(crate) fn take_streams(&mut self) -> SocketResult<(DynInputStream, DynOutputStream)> {
        if self.streams.is_some() {
            return Err(ErrorCode::InvalidState.into());
        }
        let (streams, input, output) = streams::Streams::new(self.socket()?.connection()?);
        self.streams = Some(streams);
        Ok((input, output))
    }
    pub(crate) fn shutdown(&mut self, receive: bool, send: bool) -> SocketResult<()> {
        self.streams
            .as_mut()
            .ok_or(ErrorCode::InvalidState)?
            .shutdown(receive, send)
    }
}
fn noop() -> Context<'static> {
    Context::from_waker(futures_util::task::noop_waker_ref())
}
fn readiness_error<T>(result: Option<&SocketResult<T>>) -> wasmtime::Result<()> {
    if let Some(Err(error)) = result {
        if error.downcast_ref().is_none() {
            return Err(wasmtime::format_err!("TCP native driver failed: {error}"));
        }
    }
    Ok(())
}
#[async_trait::async_trait]
impl Pollable for TcpSocket {
    async fn ready(&mut self) -> wasmtime::Result<()> {
        match self.operation {
            Some(AsyncOperation::Connect) => std::future::poll_fn(|cx| self.poll_connect(cx)).await,
            None if self.inner.as_ref().is_some_and(NativeSocket::is_listening) => {
                std::future::poll_fn(|cx| self.poll_accept(cx)).await
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
