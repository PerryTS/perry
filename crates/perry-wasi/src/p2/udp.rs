//! WASI UDP resources, bounded packet readiness and retained stream cleanup.
use crate::{
    io::poll::Pollable,
    p2::{
        bindings::sockets::udp::{IncomingDatagram, OutgoingDatagram},
        SocketError, SocketResult,
    },
    sockets::{
        tcp::Failure,
        udp::{Datagram, ReceiveTask, SendTask, UdpConnection, UdpSocket as NativeSocket},
    },
};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

pub struct UdpSocket {
    pub(crate) inner: NativeSocket,
    pub(crate) binding: bool,
    pub(crate) network: Arc<()>,
    pub(crate) cleanup: SharedCleanup,
}
impl UdpSocket {
    pub(crate) fn new(inner: NativeSocket, network: Arc<()>) -> Self {
        Self {
            inner,
            network,
            binding: false,
            cleanup: Arc::default(),
        }
    }
}
#[async_trait::async_trait]
impl Pollable for UdpSocket {
    async fn ready(&mut self) -> wasmtime::Result<()> {
        Ok(())
    }
}
pub(crate) type SharedCleanup = Arc<Mutex<Cleanup>>;
#[derive(Default)]
pub(crate) struct Cleanup {
    tasks: Vec<SendTask>,
    error: Option<perry_component_io::Error>,
}
impl Cleanup {
    fn push(&mut self, task: Result<SendTask, perry_component_io::Error>) {
        match task {
            Ok(task) => self.tasks.push(task),
            Err(error) => self.error = Some(error),
        }
    }
    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<wasmtime::Result<()>> {
        let mut index = 0;
        while index < self.tasks.len() {
            match Pin::new(&mut self.tasks[index]).poll(cx) {
                Poll::Pending => index += 1,
                Poll::Ready(result) => {
                    drop(self.tasks.swap_remove(index));
                    match result {
                        Ok(Ok(())) | Ok(Err(Failure::Socket(_) | Failure::BrokenPipe(_))) => (),
                        Ok(Err(Failure::Driver(error))) | Err(error) => self.error = Some(error),
                    }
                }
            }
        }
        if let Some(error) = &self.error {
            return Poll::Ready(Err(error.clone().into()));
        }
        if self.tasks.is_empty() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }
}
pub(crate) async fn cleanup(shared: &SharedCleanup) -> wasmtime::Result<()> {
    std::future::poll_fn(|cx| shared.lock().unwrap().poll(cx)).await
}
fn noop() -> Context<'static> {
    Context::from_waker(futures_util::task::noop_waker_ref())
}
fn ready_error(error: &Option<Failure>) -> wasmtime::Result<()> {
    if let Some(Failure::Driver(error)) = error {
        Err(error.clone().into())
    } else {
        Ok(())
    }
}
fn take_error(error: &mut Option<Failure>) -> SocketResult<()> {
    match error {
        Some(Failure::Driver(error)) => Err(SocketError::trap(error.clone())),
        Some(_) => Err(error.take().unwrap().socket()),
        None => Ok(()),
    }
}

pub struct IncomingDatagramStream {
    connection: UdpConnection,
    pending: Option<ReceiveTask>,
    packet: Option<Datagram>,
    error: Option<Failure>,
    pub(crate) cleanup: SharedCleanup,
}
impl IncomingDatagramStream {
    pub(crate) fn new(connection: UdpConnection, cleanup: SharedCleanup) -> Self {
        Self {
            connection,
            pending: None,
            packet: None,
            error: None,
            cleanup,
        }
    }
    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<wasmtime::Result<()>> {
        if self.pending.is_none() && self.packet.is_none() && self.error.is_none() {
            match self.connection.receive() {
                Ok(task) => self.pending = Some(task),
                Err(error) => self.error = Some(Failure::Driver(error)),
            }
        }
        if let Some(task) = &mut self.pending {
            let result = std::task::ready!(Pin::new(task).poll(cx));
            self.pending = None;
            match result {
                Ok(Ok(packet)) => self.packet = Some(packet),
                Ok(Err(error)) => self.error = Some(error),
                Err(error) => self.error = Some(Failure::Driver(error)),
            }
        }
        Poll::Ready(ready_error(&self.error))
    }
    pub(crate) fn receive(&mut self, maximum: u64) -> SocketResult<Vec<IncomingDatagram>> {
        if maximum == 0 {
            return Ok(Vec::new());
        }
        let _ = self.poll(&mut noop());
        take_error(&mut self.error)?;
        Ok(self
            .packet
            .take()
            .map(|packet| IncomingDatagram {
                data: packet.data,
                remote_address: packet.from.into(),
            })
            .into_iter()
            .collect())
    }
}
#[async_trait::async_trait]
impl Pollable for IncomingDatagramStream {
    async fn ready(&mut self) -> wasmtime::Result<()> {
        std::future::poll_fn(|cx| self.poll(cx)).await
    }
}
impl Drop for IncomingDatagramStream {
    fn drop(&mut self) {
        self.cleanup
            .lock()
            .unwrap()
            .push(self.connection.cancel_receive(self.pending.take()));
    }
}

pub struct OutgoingDatagramStream {
    connection: UdpConnection,
    pending: Option<SendTask>,
    permit: bool,
    error: Option<Failure>,
    pub(crate) cleanup: SharedCleanup,
}
impl OutgoingDatagramStream {
    pub(crate) fn new(connection: UdpConnection, cleanup: SharedCleanup) -> Self {
        Self {
            connection,
            pending: None,
            permit: false,
            error: None,
            cleanup,
        }
    }
    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<wasmtime::Result<()>> {
        if let Some(task) = &mut self.pending {
            let result = std::task::ready!(Pin::new(task).poll(cx));
            self.pending = None;
            match result {
                Ok(Ok(())) => (),
                Ok(Err(error)) => self.error = Some(error),
                Err(error) => self.error = Some(Failure::Driver(error)),
            }
        }
        Poll::Ready(ready_error(&self.error))
    }
    pub(crate) fn check_send(&mut self) -> SocketResult<u64> {
        let _ = self.poll(&mut noop());
        take_error(&mut self.error)?;
        self.permit = self.pending.is_none();
        Ok(u64::from(self.permit))
    }
    pub(crate) async fn send(&mut self, datagrams: Vec<OutgoingDatagram>) -> SocketResult<u64> {
        if !self.permit || datagrams.len() > 1 {
            return Err(SocketError::trap(wasmtime::format_err!(
                "UDP send requires a check-send permit and cannot exceed it"
            )));
        }
        self.permit = false;
        take_error(&mut self.error)?;
        if let Some(datagram) = datagrams.into_iter().next() {
            match self
                .connection
                .send(datagram.data, datagram.remote_address.map(Into::into))
                .await
            {
                Ok(task) => {
                    self.pending = Some(task);
                    Ok(1)
                }
                Err(error) => {
                    if error.downcast_ref().is_none() {
                        self.error = Some(Failure::Driver(perry_component_io::Error::Driver(
                            error.to_string(),
                        )));
                    }
                    Err(error)
                }
            }
        } else {
            Ok(0)
        }
    }
}
#[async_trait::async_trait]
impl Pollable for OutgoingDatagramStream {
    async fn ready(&mut self) -> wasmtime::Result<()> {
        std::future::poll_fn(|cx| self.poll(cx)).await
    }
}
impl Drop for OutgoingDatagramStream {
    fn drop(&mut self) {
        self.cleanup
            .lock()
            .unwrap()
            .push(self.connection.finish_send(self.pending.take()));
    }
}

#[cfg(test)]
mod tests;
