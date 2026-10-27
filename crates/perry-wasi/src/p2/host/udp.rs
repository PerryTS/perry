// Sync forwarding adapted from Wasmtime 48.0.5; see PROVENANCE.md.
use crate::{
    io::poll::{subscribe, DynPollable},
    p2::{
        bindings::sockets::{
            network::{ErrorCode, IpAddressFamily, IpSocketAddress, Network},
            udp, udp_create_socket,
        },
        udp::{cleanup, IncomingDatagramStream, OutgoingDatagramStream, UdpSocket},
        SocketError, SocketResult,
    },
    sockets::{udp::UdpSocket as NativeSocket, WasiSocketsCtxView},
};
use std::sync::Arc;
use wasmtime::component::Resource;

impl udp_create_socket::Host for WasiSocketsCtxView<'_> {
    fn create_udp_socket(&mut self, family: IpAddressFamily) -> SocketResult<Resource<UdpSocket>> {
        let native = NativeSocket::new(self.ctx, family.into())?;
        Ok(self
            .table
            .push(UdpSocket::new(native, self.ctx.network_identity.clone()))?)
    }
}
impl udp::Host for WasiSocketsCtxView<'_> {}
impl udp::HostUdpSocket for WasiSocketsCtxView<'_> {
    async fn start_bind(
        &mut self,
        this: Resource<UdpSocket>,
        network: Resource<Network>,
        address: IpSocketAddress,
    ) -> SocketResult<()> {
        let identity = self.table.get(&network)?.identity.clone();
        let socket = self.table.get_mut(&this)?;
        if !Arc::ptr_eq(&socket.network, &identity) {
            return Err(ErrorCode::InvalidArgument.into());
        }
        if socket.binding {
            return Err(ErrorCode::ConcurrencyConflict.into());
        }
        socket.inner.bind(address.into()).await?;
        socket.binding = true;
        Ok(())
    }
    fn finish_bind(&mut self, this: Resource<UdpSocket>) -> SocketResult<()> {
        let socket = self.table.get_mut(&this)?;
        if !socket.binding {
            return Err(ErrorCode::NotInProgress.into());
        }
        socket.binding = false;
        Ok(())
    }
    async fn stream(
        &mut self,
        this: Resource<UdpSocket>,
        remote: Option<IpSocketAddress>,
    ) -> SocketResult<(
        Resource<IncomingDatagramStream>,
        Resource<OutgoingDatagramStream>,
    )> {
        if self.table.iter_children(&this)?.any(|child| {
            child.is::<IncomingDatagramStream>() || child.is::<OutgoingDatagramStream>()
        }) {
            return Err(SocketError::trap(wasmtime::format_err!(
                "UDP streams must be dropped before reassociation"
            )));
        }
        let socket = self.table.get_mut(&this)?;
        if socket.binding {
            return Err(ErrorCode::ConcurrencyConflict.into());
        }
        cleanup(&socket.cleanup).await.map_err(SocketError::trap)?;
        let connection = socket.inner.stream(remote.map(Into::into)).await?;
        let incoming = IncomingDatagramStream::new(connection.clone(), socket.cleanup.clone());
        let outgoing = OutgoingDatagramStream::new(connection, socket.cleanup.clone());
        let incoming = self.table.push_child(incoming, &this)?;
        match self.table.push_child(outgoing, &this) {
            Ok(outgoing) => Ok((incoming, outgoing)),
            Err(error) => {
                let _ = self.table.delete(incoming);
                Err(error.into())
            }
        }
    }
    fn local_address(&mut self, this: Resource<UdpSocket>) -> SocketResult<IpSocketAddress> {
        Ok(self.table.get(&this)?.inner.local_address()?.into())
    }
    fn remote_address(&mut self, this: Resource<UdpSocket>) -> SocketResult<IpSocketAddress> {
        Ok(self.table.get(&this)?.inner.remote_address()?.into())
    }
    fn address_family(&mut self, this: Resource<UdpSocket>) -> wasmtime::Result<IpAddressFamily> {
        Ok(self.table.get(&this)?.inner.family().into())
    }
    fn unicast_hop_limit(&mut self, this: Resource<UdpSocket>) -> SocketResult<u8> {
        self.table.get(&this)?.inner.hop_limit()
    }
    fn set_unicast_hop_limit(&mut self, this: Resource<UdpSocket>, value: u8) -> SocketResult<()> {
        self.table.get_mut(&this)?.inner.set_hop_limit(value)
    }
    fn receive_buffer_size(&mut self, this: Resource<UdpSocket>) -> SocketResult<u64> {
        self.table.get(&this)?.inner.receive_buffer_size()
    }
    fn set_receive_buffer_size(
        &mut self,
        this: Resource<UdpSocket>,
        value: u64,
    ) -> SocketResult<()> {
        self.table
            .get_mut(&this)?
            .inner
            .set_receive_buffer_size(value)
    }
    fn send_buffer_size(&mut self, this: Resource<UdpSocket>) -> SocketResult<u64> {
        self.table.get(&this)?.inner.send_buffer_size()
    }
    fn set_send_buffer_size(&mut self, this: Resource<UdpSocket>, value: u64) -> SocketResult<()> {
        self.table.get_mut(&this)?.inner.set_send_buffer_size(value)
    }
    fn subscribe(&mut self, this: Resource<UdpSocket>) -> wasmtime::Result<Resource<DynPollable>> {
        subscribe(self.table, this)
    }
    fn drop(&mut self, this: Resource<UdpSocket>) -> wasmtime::Result<()> {
        self.table.delete(this)?;
        Ok(())
    }
}
impl udp::HostIncomingDatagramStream for WasiSocketsCtxView<'_> {
    fn receive(
        &mut self,
        this: Resource<IncomingDatagramStream>,
        maximum: u64,
    ) -> SocketResult<Vec<udp::IncomingDatagram>> {
        self.table.get_mut(&this)?.receive(maximum)
    }
    fn subscribe(
        &mut self,
        this: Resource<IncomingDatagramStream>,
    ) -> wasmtime::Result<Resource<DynPollable>> {
        subscribe(self.table, this)
    }
    async fn drop(&mut self, this: Resource<IncomingDatagramStream>) -> wasmtime::Result<()> {
        let stream = self.table.delete(this)?;
        let tasks = stream.cleanup.clone();
        drop(stream);
        cleanup(&tasks).await
    }
}
impl udp::HostOutgoingDatagramStream for WasiSocketsCtxView<'_> {
    fn check_send(&mut self, this: Resource<OutgoingDatagramStream>) -> SocketResult<u64> {
        self.table.get_mut(&this)?.check_send()
    }
    async fn send(
        &mut self,
        this: Resource<OutgoingDatagramStream>,
        datagrams: Vec<udp::OutgoingDatagram>,
    ) -> SocketResult<u64> {
        self.table.get_mut(&this)?.send(datagrams).await
    }
    fn subscribe(
        &mut self,
        this: Resource<OutgoingDatagramStream>,
    ) -> wasmtime::Result<Resource<DynPollable>> {
        subscribe(self.table, this)
    }
    async fn drop(&mut self, this: Resource<OutgoingDatagramStream>) -> wasmtime::Result<()> {
        let stream = self.table.delete(this)?;
        let tasks = stream.cleanup.clone();
        drop(stream);
        cleanup(&tasks).await
    }
}

pub mod sync {
    use wasmtime::component::Resource;

    use crate::p2::{
        bindings::{
            sockets::{
                network::Network,
                udp::{
                    self as async_udp,
                    HostIncomingDatagramStream as AsyncHostIncomingDatagramStream,
                    HostOutgoingDatagramStream as AsyncHostOutgoingDatagramStream,
                    HostUdpSocket as AsyncHostUdpSocket, IncomingDatagramStream,
                    OutgoingDatagramStream,
                },
            },
            sync::sockets::udp::{
                self, HostIncomingDatagramStream, HostOutgoingDatagramStream, HostUdpSocket,
                IncomingDatagram, IpAddressFamily, IpSocketAddress, OutgoingDatagram, Pollable,
            },
        },
        udp::UdpSocket,
        SocketError,
    };
    use crate::sockets::WasiSocketsCtxView;
    use futures_lite::future::block_on;

    impl udp::Host for WasiSocketsCtxView<'_> {}

    impl HostUdpSocket for WasiSocketsCtxView<'_> {
        fn start_bind(
            &mut self,
            self_: Resource<UdpSocket>,
            network: Resource<Network>,
            local_address: IpSocketAddress,
        ) -> Result<(), SocketError> {
            block_on(async {
                AsyncHostUdpSocket::start_bind(self, self_, network, local_address).await
            })
        }

        fn finish_bind(&mut self, self_: Resource<UdpSocket>) -> Result<(), SocketError> {
            AsyncHostUdpSocket::finish_bind(self, self_)
        }

        fn stream(
            &mut self,
            self_: Resource<UdpSocket>,
            remote_address: Option<IpSocketAddress>,
        ) -> Result<
            (
                Resource<IncomingDatagramStream>,
                Resource<OutgoingDatagramStream>,
            ),
            SocketError,
        > {
            block_on(async { AsyncHostUdpSocket::stream(self, self_, remote_address).await })
        }

        fn local_address(
            &mut self,
            self_: Resource<UdpSocket>,
        ) -> Result<IpSocketAddress, SocketError> {
            AsyncHostUdpSocket::local_address(self, self_)
        }

        fn remote_address(
            &mut self,
            self_: Resource<UdpSocket>,
        ) -> Result<IpSocketAddress, SocketError> {
            AsyncHostUdpSocket::remote_address(self, self_)
        }

        fn address_family(
            &mut self,
            self_: Resource<UdpSocket>,
        ) -> wasmtime::Result<IpAddressFamily> {
            AsyncHostUdpSocket::address_family(self, self_)
        }

        fn unicast_hop_limit(&mut self, self_: Resource<UdpSocket>) -> Result<u8, SocketError> {
            AsyncHostUdpSocket::unicast_hop_limit(self, self_)
        }

        fn set_unicast_hop_limit(
            &mut self,
            self_: Resource<UdpSocket>,
            value: u8,
        ) -> Result<(), SocketError> {
            AsyncHostUdpSocket::set_unicast_hop_limit(self, self_, value)
        }

        fn receive_buffer_size(&mut self, self_: Resource<UdpSocket>) -> Result<u64, SocketError> {
            AsyncHostUdpSocket::receive_buffer_size(self, self_)
        }

        fn set_receive_buffer_size(
            &mut self,
            self_: Resource<UdpSocket>,
            value: u64,
        ) -> Result<(), SocketError> {
            AsyncHostUdpSocket::set_receive_buffer_size(self, self_, value)
        }

        fn send_buffer_size(&mut self, self_: Resource<UdpSocket>) -> Result<u64, SocketError> {
            AsyncHostUdpSocket::send_buffer_size(self, self_)
        }

        fn set_send_buffer_size(
            &mut self,
            self_: Resource<UdpSocket>,
            value: u64,
        ) -> Result<(), SocketError> {
            AsyncHostUdpSocket::set_send_buffer_size(self, self_, value)
        }

        fn subscribe(
            &mut self,
            self_: Resource<UdpSocket>,
        ) -> wasmtime::Result<Resource<Pollable>> {
            AsyncHostUdpSocket::subscribe(self, self_)
        }

        fn drop(&mut self, rep: Resource<UdpSocket>) -> wasmtime::Result<()> {
            AsyncHostUdpSocket::drop(self, rep)
        }
    }

    impl HostIncomingDatagramStream for WasiSocketsCtxView<'_> {
        fn receive(
            &mut self,
            self_: Resource<IncomingDatagramStream>,
            max_results: u64,
        ) -> Result<Vec<IncomingDatagram>, SocketError> {
            Ok(
                AsyncHostIncomingDatagramStream::receive(self, self_, max_results)?
                    .into_iter()
                    .map(Into::into)
                    .collect(),
            )
        }

        fn subscribe(
            &mut self,
            self_: Resource<IncomingDatagramStream>,
        ) -> wasmtime::Result<Resource<Pollable>> {
            AsyncHostIncomingDatagramStream::subscribe(self, self_)
        }

        fn drop(&mut self, rep: Resource<IncomingDatagramStream>) -> wasmtime::Result<()> {
            block_on(async { AsyncHostIncomingDatagramStream::drop(self, rep).await })
        }
    }

    impl From<async_udp::IncomingDatagram> for IncomingDatagram {
        fn from(other: async_udp::IncomingDatagram) -> Self {
            let async_udp::IncomingDatagram {
                data,
                remote_address,
            } = other;
            Self {
                data,
                remote_address,
            }
        }
    }

    impl HostOutgoingDatagramStream for WasiSocketsCtxView<'_> {
        fn check_send(
            &mut self,
            self_: Resource<OutgoingDatagramStream>,
        ) -> Result<u64, SocketError> {
            AsyncHostOutgoingDatagramStream::check_send(self, self_)
        }

        fn send(
            &mut self,
            self_: Resource<OutgoingDatagramStream>,
            datagrams: Vec<OutgoingDatagram>,
        ) -> Result<u64, SocketError> {
            let datagrams = datagrams.into_iter().map(Into::into).collect();
            block_on(async { AsyncHostOutgoingDatagramStream::send(self, self_, datagrams).await })
        }

        fn subscribe(
            &mut self,
            self_: Resource<OutgoingDatagramStream>,
        ) -> wasmtime::Result<Resource<Pollable>> {
            AsyncHostOutgoingDatagramStream::subscribe(self, self_)
        }

        fn drop(&mut self, rep: Resource<OutgoingDatagramStream>) -> wasmtime::Result<()> {
            block_on(async { AsyncHostOutgoingDatagramStream::drop(self, rep).await })
        }
    }

    impl From<OutgoingDatagram> for async_udp::OutgoingDatagram {
        fn from(other: OutgoingDatagram) -> Self {
            let OutgoingDatagram {
                data,
                remote_address,
            } = other;
            Self {
                data,
                remote_address,
            }
        }
    }
}
