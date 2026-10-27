//! Concurrent datagrams serialize each direction independently. Cancellation
//! retains cleanup acknowledgements so the next call cannot steal a completion.
use super::{slot::Slot, types::*, SocketResult};
use crate::sockets::{
    tcp::Family,
    udp::{ReceiveTask, SendTask, UdpConnection, UdpSocket as Native},
    WasiSockets, WasiSocketsCtxView,
};
use futures_util::lock::{Mutex as AsyncMutex, MutexGuard};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use wasmtime::component::{Accessor, Resource};

pub struct UdpSocket {
    native: Slot<Native>,
    family: Family,
    configuration: Arc<AsyncMutex<()>>,
    remote: Arc<Mutex<Option<SocketAddr>>>,
    incoming: Arc<AsyncMutex<Incoming>>,
    outgoing: Arc<AsyncMutex<Outgoing>>,
}
#[derive(Default)]
struct Incoming {
    cleanup: Option<SendTask>,
}
#[derive(Default)]
struct Outgoing {
    cleanup: Option<SendTask>,
}
struct Read<'a> {
    state: MutexGuard<'a, Incoming>,
    connection: UdpConnection,
    pending: Option<ReceiveTask>,
}
impl Drop for Read<'_> {
    fn drop(&mut self) {
        if self.pending.is_some() {
            self.state.cleanup = self.connection.cancel_receive(self.pending.take()).ok();
        }
    }
}
struct Write<'a> {
    state: MutexGuard<'a, Outgoing>,
    connection: UdpConnection,
    pending: Option<SendTask>,
}
impl Drop for Write<'_> {
    fn drop(&mut self) {
        if self.pending.is_some() {
            self.state.cleanup = self.connection.finish_send(self.pending.take()).ok();
        }
    }
}
async fn cleanup(task: Option<SendTask>) -> SocketResult<()> {
    if let Some(task) = task {
        task.await??;
    }
    Ok(())
}
impl<T> HostUdpSocketWithStore<T> for WasiSockets {
    async fn send(
        store: &Accessor<T, Self>,
        socket: Resource<UdpSocket>,
        data: Vec<u8>,
        address: Option<IpSocketAddress>,
    ) -> SocketResult<()> {
        let (native, configuration, outgoing) = store.with(|mut s| {
            let socket = s.get().table.get(&socket)?;
            SocketResult::Ok((
                socket.native.clone(),
                socket.configuration.clone(),
                socket.outgoing.clone(),
            ))
        })?;
        let mut state = outgoing.lock().await;
        cleanup(state.cleanup.take()).await?;
        let (connection, pending) = {
            let _configuration = configuration.lock().await;
            let mut loan = native.take(true)?;
            loan.get()
                .prepare_send(data, address.map(Into::into))
                .await?
        };
        let mut write = Write {
            state,
            connection,
            pending: Some(pending),
        };
        let result = write.pending.as_mut().unwrap().await;
        write.pending = None;
        result??;
        Ok(())
    }
    async fn receive(
        store: &Accessor<T, Self>,
        socket: Resource<UdpSocket>,
    ) -> SocketResult<(Vec<u8>, IpSocketAddress)> {
        let (native, configuration, incoming, remote) = store.with(|mut s| {
            let socket = s.get().table.get(&socket)?;
            SocketResult::Ok((
                socket.native.clone(),
                socket.configuration.clone(),
                socket.incoming.clone(),
                socket.remote.clone(),
            ))
        })?;
        let mut state = incoming.lock().await;
        cleanup(state.cleanup.take()).await?;
        let connection = {
            let _configuration = configuration.lock().await;
            native.with(Native::unfiltered_connection)?
        };
        let mut read = Read {
            state,
            connection,
            pending: None,
        };
        let mut discarded = 0;
        loop {
            read.pending = Some(read.connection.receive()?);
            let result = read.pending.as_mut().unwrap().await;
            read.pending = None;
            let packet = result??;
            // Check the current association at delivery, so a concurrent
            // connect/disconnect also applies to an already pending receive.
            if remote
                .lock()
                .unwrap()
                .is_none_or(|peer| peer == packet.from)
            {
                return Ok((packet.data, packet.from.into()));
            }
            discarded += 1;
            if discarded == 16 {
                futures_lite::future::yield_now().await;
                discarded = 0;
            }
        }
    }
}
impl HostUdpSocket for WasiSocketsCtxView<'_> {
    fn create(&mut self, family: IpAddressFamily) -> SocketResult<Resource<UdpSocket>> {
        let family = family.into();
        let native = Native::new(self.ctx, family)?;
        Ok(self.table.push(UdpSocket {
            native: Slot::new(native),
            family,
            configuration: Arc::new(AsyncMutex::new(())),
            remote: Arc::new(Mutex::new(None)),
            incoming: Arc::new(AsyncMutex::new(Incoming::default())),
            outgoing: Arc::new(AsyncMutex::new(Outgoing::default())),
        })?)
    }
    async fn bind(
        &mut self,
        socket: Resource<UdpSocket>,
        address: IpSocketAddress,
    ) -> SocketResult<()> {
        let socket = self.table.get(&socket)?;
        let _configuration = socket.configuration.lock().await;
        let mut loan = socket.native.take(true)?;
        loan.get().bind(address.into()).await?;
        Ok(())
    }
    async fn connect(
        &mut self,
        socket: Resource<UdpSocket>,
        address: IpSocketAddress,
    ) -> SocketResult<()> {
        let socket = self.table.get(&socket)?;
        let _configuration = socket.configuration.lock().await;
        let mut loan = socket.native.take(true)?;
        let address = address.into();
        loan.get().connect(address).await?;
        *socket.remote.lock().unwrap() = Some(address);
        Ok(())
    }
    fn disconnect(&mut self, socket: Resource<UdpSocket>) -> SocketResult<()> {
        let socket = self.table.get(&socket)?;
        let _configuration = socket
            .configuration
            .try_lock()
            .ok_or(ErrorCode::InvalidState)?;
        socket.native.with_mut(Native::disconnect)?;
        *socket.remote.lock().unwrap() = None;
        Ok(())
    }
    fn get_address_family(
        &mut self,
        socket: Resource<UdpSocket>,
    ) -> wasmtime::Result<IpAddressFamily> {
        Ok(self.table.get(&socket)?.family.into())
    }
    fn drop(&mut self, socket: Resource<UdpSocket>) -> wasmtime::Result<()> {
        self.table.delete(socket)?;
        Ok(())
    }
    fn get_local_address(&mut self, socket: Resource<UdpSocket>) -> SocketResult<IpSocketAddress> {
        self.table
            .get(&socket)?
            .native
            .with(Native::local_address)
            .map(Into::into)
    }
    fn get_remote_address(&mut self, socket: Resource<UdpSocket>) -> SocketResult<IpSocketAddress> {
        self.table
            .get(&socket)?
            .native
            .with(Native::remote_address)
            .map(Into::into)
    }
    fn get_unicast_hop_limit(&mut self, socket: Resource<UdpSocket>) -> SocketResult<u8> {
        self.table.get(&socket)?.native.with(Native::hop_limit)
    }
    fn get_receive_buffer_size(&mut self, socket: Resource<UdpSocket>) -> SocketResult<u64> {
        self.table
            .get(&socket)?
            .native
            .with(Native::receive_buffer_size)
    }
    fn get_send_buffer_size(&mut self, socket: Resource<UdpSocket>) -> SocketResult<u64> {
        self.table
            .get(&socket)?
            .native
            .with(Native::send_buffer_size)
    }
    fn set_unicast_hop_limit(
        &mut self,
        socket: Resource<UdpSocket>,
        value: u8,
    ) -> SocketResult<()> {
        self.table
            .get(&socket)?
            .native
            .with_mut(|s| s.set_hop_limit(value))
    }
    fn set_receive_buffer_size(
        &mut self,
        socket: Resource<UdpSocket>,
        value: u64,
    ) -> SocketResult<()> {
        self.table
            .get(&socket)?
            .native
            .with_mut(|s| s.set_receive_buffer_size(value))
    }
    fn set_send_buffer_size(
        &mut self,
        socket: Resource<UdpSocket>,
        value: u64,
    ) -> SocketResult<()> {
        self.table
            .get(&socket)?
            .native
            .with_mut(|s| s.set_send_buffer_size(value))
    }
}
