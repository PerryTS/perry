//! P3 TCP resources own native transports independently of canonical streams.
mod streams;
use super::{slot::Slot, types::*, SocketError, SocketResult};
use crate::sockets::{
    tcp::{Family, TcpSocket as Native},
    WasiSockets, WasiSocketsCtxView,
};
use futures_channel::oneshot;
use wasmtime::component::{Access, Accessor, FutureReader, Resource, StreamReader};

pub struct TcpSocket {
    native: Slot<Native>,
    family: Family,
    sent: bool,
    received: bool,
}
impl TcpSocket {
    fn new(native: Native) -> Self {
        Self {
            family: native.family(),
            native: Slot::new(native),
            sent: false,
            received: false,
        }
    }
}
impl<T: Send + 'static> HostTcpSocketWithStore<T> for WasiSockets {
    async fn connect(
        store: &Accessor<T, Self>,
        socket: Resource<TcpSocket>,
        address: IpSocketAddress,
    ) -> SocketResult<()> {
        let address = address.into();
        let mut loan = store.with(|mut s| {
            let socket = s.get().table.get(&socket)?;
            socket.native.with(|native| native.check_connect(address))?;
            let control = socket.native.with(Native::connecting_view)?;
            let mut loan = socket.native.take(false)?;
            loan.expose(control);
            SocketResult::Ok(loan)
        })?;
        loan.get().connect(address).await?;
        loan.restore();
        Ok(())
    }
    async fn listen(
        mut store: Access<'_, T, Self>,
        socket: Resource<TcpSocket>,
    ) -> SocketResult<StreamReader<Resource<TcpSocket>>> {
        let native = store.get().table.get(&socket)?.native.clone();
        let mut loan = native.take(true)?;
        let socket = loan.get();
        if !socket.is_bound() {
            socket.bind(socket.family().unspecified()).await?;
        }
        socket.listen().await?;
        drop(loan);
        let getter = store.getter();
        StreamReader::new(&mut store, streams::Listener::new(native, getter))
            .map_err(SocketError::trap)
    }
    fn send(
        mut store: Access<'_, T, Self>,
        socket: Resource<TcpSocket>,
        mut data: StreamReader<u8>,
    ) -> wasmtime::Result<FutureReader<Result<(), ErrorCode>>> {
        let socket = store.get().table.get_mut(&socket)?;
        let connection = if socket.sent {
            Err(ErrorCode::InvalidState.into())
        } else {
            socket.native.with(Native::connection)
        };
        let connection = match connection {
            Ok(connection) => {
                socket.sent = true;
                connection
            }
            Err(error) => {
                let code = error.downcast()?;
                data.close(&mut store)?;
                return FutureReader::new(
                    &mut store,
                    async move { wasmtime::error::Ok(Err(code)) },
                );
            }
        };
        let (send, receive) = oneshot::channel();
        data.pipe(&mut store, streams::Output::new(connection, send))?;
        FutureReader::new(
            &mut store,
            async move { streams::finish_output(receive).await },
        )
    }
    fn receive(
        mut store: Access<'_, T, Self>,
        socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<(StreamReader<u8>, FutureReader<Result<(), ErrorCode>>)> {
        let socket = store.get().table.get_mut(&socket)?;
        let connection = if socket.received {
            Err(ErrorCode::InvalidState.into())
        } else {
            socket.native.with(Native::connection)
        };
        let connection = match connection {
            Ok(connection) => {
                socket.received = true;
                connection
            }
            Err(error) => {
                let code = error.downcast()?;
                return Ok((
                    StreamReader::new(&mut store, std::iter::empty())?,
                    FutureReader::new(&mut store, async move { wasmtime::error::Ok(Err(code)) })?,
                ));
            }
        };
        let (send, receive) = oneshot::channel();
        Ok((
            StreamReader::new(&mut store, streams::Input::new(connection, send))?,
            FutureReader::new(&mut store, async move {
                wasmtime::error::Ok(receive.await.unwrap_or(Ok(())))
            })?,
        ))
    }
}
impl HostTcpSocket for WasiSocketsCtxView<'_> {
    fn create(&mut self, family: IpAddressFamily) -> SocketResult<Resource<TcpSocket>> {
        let native = Native::new(self.ctx, family.into())?;
        Ok(self.table.push(TcpSocket::new(native))?)
    }
    async fn bind(
        &mut self,
        socket: Resource<TcpSocket>,
        address: IpSocketAddress,
    ) -> SocketResult<()> {
        let mut loan = self.table.get(&socket)?.native.take(true)?;
        loan.get().bind(address.into()).await?;
        Ok(())
    }
    fn get_address_family(
        &mut self,
        socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<IpAddressFamily> {
        Ok(self.table.get(&socket)?.family.into())
    }
    fn get_is_listening(&mut self, socket: Resource<TcpSocket>) -> wasmtime::Result<bool> {
        let socket = self.table.get(&socket)?;
        match socket.native.with(|s| Ok(s.is_listening())) {
            Ok(value) => Ok(value),
            Err(error) => match error.downcast() {
                Ok(_) => Ok(false),
                Err(error) => Err(error),
            },
        }
    }
    fn drop(&mut self, socket: Resource<TcpSocket>) -> wasmtime::Result<()> {
        self.table.delete(socket)?;
        Ok(())
    }
    fn get_local_address(&mut self, socket: Resource<TcpSocket>) -> SocketResult<IpSocketAddress> {
        self.table
            .get(&socket)?
            .native
            .with(Native::local_address)
            .map(Into::into)
    }
    fn get_remote_address(&mut self, socket: Resource<TcpSocket>) -> SocketResult<IpSocketAddress> {
        self.table
            .get(&socket)?
            .native
            .with(Native::remote_address)
            .map(Into::into)
    }
    fn get_keep_alive_enabled(&mut self, socket: Resource<TcpSocket>) -> SocketResult<bool> {
        self.table
            .get(&socket)?
            .native
            .with(Native::keep_alive_enabled)
    }
    fn get_keep_alive_idle_time(&mut self, socket: Resource<TcpSocket>) -> SocketResult<u64> {
        self.table
            .get(&socket)?
            .native
            .with(Native::keep_alive_idle_time)
    }
    fn get_keep_alive_interval(&mut self, socket: Resource<TcpSocket>) -> SocketResult<u64> {
        self.table
            .get(&socket)?
            .native
            .with(Native::keep_alive_interval)
    }
    fn get_keep_alive_count(&mut self, socket: Resource<TcpSocket>) -> SocketResult<u32> {
        self.table
            .get(&socket)?
            .native
            .with(Native::keep_alive_count)
    }
    fn get_hop_limit(&mut self, socket: Resource<TcpSocket>) -> SocketResult<u8> {
        self.table.get(&socket)?.native.with(Native::hop_limit)
    }
    fn get_receive_buffer_size(&mut self, socket: Resource<TcpSocket>) -> SocketResult<u64> {
        self.table
            .get(&socket)?
            .native
            .with(Native::receive_buffer_size)
    }
    fn get_send_buffer_size(&mut self, socket: Resource<TcpSocket>) -> SocketResult<u64> {
        self.table
            .get(&socket)?
            .native
            .with(Native::send_buffer_size)
    }
    fn set_listen_backlog_size(
        &mut self,
        socket: Resource<TcpSocket>,
        value: u64,
    ) -> SocketResult<()> {
        self.table
            .get(&socket)?
            .native
            .with_mut(|s| s.set_listen_backlog_size(value))
    }
    fn set_keep_alive_enabled(
        &mut self,
        socket: Resource<TcpSocket>,
        value: bool,
    ) -> SocketResult<()> {
        self.table
            .get(&socket)?
            .native
            .with_mut(|s| s.set_keep_alive_enabled(value))
    }
    fn set_keep_alive_idle_time(
        &mut self,
        socket: Resource<TcpSocket>,
        value: u64,
    ) -> SocketResult<()> {
        self.table
            .get(&socket)?
            .native
            .with_mut(|s| s.set_keep_alive_idle_time(value))
    }
    fn set_keep_alive_interval(
        &mut self,
        socket: Resource<TcpSocket>,
        value: u64,
    ) -> SocketResult<()> {
        self.table
            .get(&socket)?
            .native
            .with_mut(|s| s.set_keep_alive_interval(value))
    }
    fn set_keep_alive_count(
        &mut self,
        socket: Resource<TcpSocket>,
        value: u32,
    ) -> SocketResult<()> {
        self.table
            .get(&socket)?
            .native
            .with_mut(|s| s.set_keep_alive_count(value))
    }
    fn set_hop_limit(&mut self, socket: Resource<TcpSocket>, value: u8) -> SocketResult<()> {
        self.table
            .get(&socket)?
            .native
            .with_mut(|s| s.set_hop_limit(value))
    }
    fn set_receive_buffer_size(
        &mut self,
        socket: Resource<TcpSocket>,
        value: u64,
    ) -> SocketResult<()> {
        self.table
            .get(&socket)?
            .native
            .with_mut(|s| s.set_receive_buffer_size(value))
    }
    fn set_send_buffer_size(
        &mut self,
        socket: Resource<TcpSocket>,
        value: u64,
    ) -> SocketResult<()> {
        self.table
            .get(&socket)?
            .native
            .with_mut(|s| s.set_send_buffer_size(value))
    }
}
