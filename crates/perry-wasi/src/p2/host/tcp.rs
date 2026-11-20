// Adapted from Wasmtime 48.0.5; Apache-2.0 WITH LLVM-exception. See PROVENANCE.md.
use crate::{
    io::poll::{subscribe, DynPollable},
    p2::{
        bindings::sockets::{
            network::{ErrorCode, IpAddressFamily, IpSocketAddress, Network},
            tcp::{self, ShutdownType},
            tcp_create_socket,
        },
        tcp::{AsyncOperation, TcpSocket},
        SocketResult,
    },
    sockets::{
        tcp::{Family, TcpSocket as NativeSocket},
        WasiSocketsCtxView,
    },
};
use wasmtime::component::{Resource, ResourceTable};
use wasmtime_wasi_io::streams::{DynInputStream, DynOutputStream};

impl tcp_create_socket::Host for WasiSocketsCtxView<'_> {
    fn create_tcp_socket(&mut self, family: IpAddressFamily) -> SocketResult<Resource<TcpSocket>> {
        let native = NativeSocket::new(self.ctx, family.into())?;
        Ok(self
            .table
            .push(TcpSocket::new(native, self.ctx.network_identity.clone()))?)
    }
}
impl From<IpAddressFamily> for Family {
    fn from(value: IpAddressFamily) -> Self {
        match value {
            IpAddressFamily::Ipv4 => Self::Ipv4,
            IpAddressFamily::Ipv6 => Self::Ipv6,
        }
    }
}
impl From<Family> for IpAddressFamily {
    fn from(value: Family) -> Self {
        match value {
            Family::Ipv4 => Self::Ipv4,
            Family::Ipv6 => Self::Ipv6,
        }
    }
}
impl tcp::Host for WasiSocketsCtxView<'_> {}
impl tcp::HostTcpSocket for WasiSocketsCtxView<'_> {
    async fn start_bind(
        &mut self,
        this: Resource<TcpSocket>,
        network: Resource<Network>,
        address: IpSocketAddress,
    ) -> SocketResult<()> {
        let identity = self.table.get(&network)?.identity.clone();
        let socket = self.table.get_mut(&this)?;
        socket.check_network(&identity)?;
        socket.idle()?;
        socket.socket_mut()?.bind(address.into()).await?;
        socket.operation = Some(AsyncOperation::Bind);
        Ok(())
    }
    fn finish_bind(&mut self, this: Resource<TcpSocket>) -> SocketResult<()> {
        let socket = self.table.get_mut(&this)?;
        if socket.operation != Some(AsyncOperation::Bind) {
            return Err(ErrorCode::NotInProgress.into());
        }
        socket.operation = None;
        Ok(())
    }
    fn start_connect(
        &mut self,
        this: Resource<TcpSocket>,
        network: Resource<Network>,
        address: IpSocketAddress,
    ) -> SocketResult<()> {
        let identity = self.table.get(&network)?.identity.clone();
        let socket = self.table.get_mut(&this)?;
        socket.check_network(&identity)?;
        socket.start_connect(address.into())
    }
    fn finish_connect(
        &mut self,
        this: Resource<TcpSocket>,
    ) -> SocketResult<(Resource<DynInputStream>, Resource<DynOutputStream>)> {
        let (input, output) = self.table.get_mut(&this)?.finish_connect()?;
        push_streams(self.table, &this, input, output)
    }
    async fn start_listen(&mut self, this: Resource<TcpSocket>) -> SocketResult<()> {
        let socket = self.table.get_mut(&this)?;
        socket.idle()?;
        socket.socket_mut()?.listen().await?;
        socket.operation = Some(AsyncOperation::Listen);
        Ok(())
    }
    fn finish_listen(&mut self, this: Resource<TcpSocket>) -> SocketResult<()> {
        let socket = self.table.get_mut(&this)?;
        if socket.operation != Some(AsyncOperation::Listen) {
            return Err(ErrorCode::NotInProgress.into());
        }
        socket.operation = None;
        Ok(())
    }
    fn accept(
        &mut self,
        this: Resource<TcpSocket>,
    ) -> SocketResult<(
        Resource<TcpSocket>,
        Resource<DynInputStream>,
        Resource<DynOutputStream>,
    )> {
        let mut child = self.table.get_mut(&this)?.accept()?;
        let (input, output) = child.take_streams()?;
        let child = self.table.push(child)?;
        match push_streams(self.table, &child, input, output) {
            Ok((input, output)) => Ok((child, input, output)),
            Err(error) => {
                let _ = self.table.delete(child);
                Err(error)
            }
        }
    }
    fn local_address(&mut self, this: Resource<TcpSocket>) -> SocketResult<IpSocketAddress> {
        Ok(self.table.get(&this)?.socket()?.local_address()?.into())
    }
    fn remote_address(&mut self, this: Resource<TcpSocket>) -> SocketResult<IpSocketAddress> {
        Ok(self.table.get(&this)?.socket()?.remote_address()?.into())
    }
    fn is_listening(&mut self, this: Resource<TcpSocket>) -> wasmtime::Result<bool> {
        Ok(self
            .table
            .get(&this)?
            .inner
            .as_ref()
            .is_some_and(NativeSocket::is_listening))
    }
    fn address_family(&mut self, this: Resource<TcpSocket>) -> wasmtime::Result<IpAddressFamily> {
        Ok(self.table.get(&this)?.family.into())
    }
    fn subscribe(&mut self, this: Resource<TcpSocket>) -> wasmtime::Result<Resource<DynPollable>> {
        subscribe(self.table, this)
    }
    fn shutdown(&mut self, this: Resource<TcpSocket>, how: ShutdownType) -> SocketResult<()> {
        self.table.get_mut(&this)?.shutdown(
            matches!(how, ShutdownType::Receive | ShutdownType::Both),
            matches!(how, ShutdownType::Send | ShutdownType::Both),
        )
    }
    fn drop(&mut self, this: Resource<TcpSocket>) -> wasmtime::Result<()> {
        self.table.delete(this)?;
        Ok(())
    }
    fn keep_alive_enabled(&mut self, this: Resource<TcpSocket>) -> SocketResult<bool> {
        self.table.get(&this)?.socket()?.keep_alive_enabled()
    }
    fn keep_alive_idle_time(&mut self, this: Resource<TcpSocket>) -> SocketResult<u64> {
        self.table.get(&this)?.socket()?.keep_alive_idle_time()
    }
    fn keep_alive_interval(&mut self, this: Resource<TcpSocket>) -> SocketResult<u64> {
        self.table.get(&this)?.socket()?.keep_alive_interval()
    }
    fn keep_alive_count(&mut self, this: Resource<TcpSocket>) -> SocketResult<u32> {
        self.table.get(&this)?.socket()?.keep_alive_count()
    }
    fn hop_limit(&mut self, this: Resource<TcpSocket>) -> SocketResult<u8> {
        self.table.get(&this)?.socket()?.hop_limit()
    }
    fn receive_buffer_size(&mut self, this: Resource<TcpSocket>) -> SocketResult<u64> {
        self.table.get(&this)?.socket()?.receive_buffer_size()
    }
    fn send_buffer_size(&mut self, this: Resource<TcpSocket>) -> SocketResult<u64> {
        self.table.get(&this)?.socket()?.send_buffer_size()
    }
    fn set_listen_backlog_size(
        &mut self,
        this: Resource<TcpSocket>,
        value: u64,
    ) -> SocketResult<()> {
        self.table
            .get_mut(&this)?
            .socket_mut()?
            .set_listen_backlog_size(value)
    }
    fn set_keep_alive_enabled(
        &mut self,
        this: Resource<TcpSocket>,
        value: bool,
    ) -> SocketResult<()> {
        self.table
            .get_mut(&this)?
            .socket_mut()?
            .set_keep_alive_enabled(value)
    }
    fn set_keep_alive_idle_time(
        &mut self,
        this: Resource<TcpSocket>,
        value: u64,
    ) -> SocketResult<()> {
        self.table
            .get_mut(&this)?
            .socket_mut()?
            .set_keep_alive_idle_time(value)
    }
    fn set_keep_alive_interval(
        &mut self,
        this: Resource<TcpSocket>,
        value: u64,
    ) -> SocketResult<()> {
        self.table
            .get_mut(&this)?
            .socket_mut()?
            .set_keep_alive_interval(value)
    }
    fn set_keep_alive_count(&mut self, this: Resource<TcpSocket>, value: u32) -> SocketResult<()> {
        self.table
            .get_mut(&this)?
            .socket_mut()?
            .set_keep_alive_count(value)
    }
    fn set_hop_limit(&mut self, this: Resource<TcpSocket>, value: u8) -> SocketResult<()> {
        self.table
            .get_mut(&this)?
            .socket_mut()?
            .set_hop_limit(value)
    }
    fn set_receive_buffer_size(
        &mut self,
        this: Resource<TcpSocket>,
        value: u64,
    ) -> SocketResult<()> {
        self.table
            .get_mut(&this)?
            .socket_mut()?
            .set_receive_buffer_size(value)
    }
    fn set_send_buffer_size(&mut self, this: Resource<TcpSocket>, value: u64) -> SocketResult<()> {
        self.table
            .get_mut(&this)?
            .socket_mut()?
            .set_send_buffer_size(value)
    }
}
fn push_streams(
    table: &mut ResourceTable,
    parent: &Resource<TcpSocket>,
    input: DynInputStream,
    output: DynOutputStream,
) -> SocketResult<(Resource<DynInputStream>, Resource<DynOutputStream>)> {
    let input = table.push_child(input, parent)?;
    match table.push_child(output, parent) {
        Ok(output) => Ok((input, output)),
        Err(error) => {
            let _ = table.delete(input);
            Err(error.into())
        }
    }
}

pub mod sync {
    use crate::p2::{
        bindings::{
            sockets::{
                network::Network,
                tcp::{self as async_tcp, HostTcpSocket as AsyncHostTcpSocket},
            },
            sync::sockets::tcp::{
                self, Duration, HostTcpSocket, InputStream, IpAddressFamily, IpSocketAddress,
                OutputStream, Pollable, ShutdownType, TcpSocket,
            },
        },
        SocketError,
    };
    use crate::sockets::WasiSocketsCtxView;
    use futures_lite::future::block_on;
    use wasmtime::component::Resource;

    impl tcp::Host for WasiSocketsCtxView<'_> {}

    impl HostTcpSocket for WasiSocketsCtxView<'_> {
        fn start_bind(
            &mut self,
            self_: Resource<TcpSocket>,
            network: Resource<Network>,
            local_address: IpSocketAddress,
        ) -> Result<(), SocketError> {
            block_on(async {
                AsyncHostTcpSocket::start_bind(self, self_, network, local_address).await
            })
        }

        fn finish_bind(&mut self, self_: Resource<TcpSocket>) -> Result<(), SocketError> {
            AsyncHostTcpSocket::finish_bind(self, self_)
        }

        fn start_connect(
            &mut self,
            self_: Resource<TcpSocket>,
            network: Resource<Network>,
            remote_address: IpSocketAddress,
        ) -> Result<(), SocketError> {
            AsyncHostTcpSocket::start_connect(self, self_, network, remote_address)
        }

        fn finish_connect(
            &mut self,
            self_: Resource<TcpSocket>,
        ) -> Result<(Resource<InputStream>, Resource<OutputStream>), SocketError> {
            AsyncHostTcpSocket::finish_connect(self, self_)
        }

        fn start_listen(&mut self, self_: Resource<TcpSocket>) -> Result<(), SocketError> {
            block_on(async { AsyncHostTcpSocket::start_listen(self, self_).await })
        }

        fn finish_listen(&mut self, self_: Resource<TcpSocket>) -> Result<(), SocketError> {
            AsyncHostTcpSocket::finish_listen(self, self_)
        }

        fn accept(
            &mut self,
            self_: Resource<TcpSocket>,
        ) -> Result<
            (
                Resource<TcpSocket>,
                Resource<InputStream>,
                Resource<OutputStream>,
            ),
            SocketError,
        > {
            AsyncHostTcpSocket::accept(self, self_)
        }

        fn local_address(
            &mut self,
            self_: Resource<TcpSocket>,
        ) -> Result<IpSocketAddress, SocketError> {
            AsyncHostTcpSocket::local_address(self, self_)
        }

        fn remote_address(
            &mut self,
            self_: Resource<TcpSocket>,
        ) -> Result<IpSocketAddress, SocketError> {
            AsyncHostTcpSocket::remote_address(self, self_)
        }

        fn is_listening(&mut self, self_: Resource<TcpSocket>) -> wasmtime::Result<bool> {
            AsyncHostTcpSocket::is_listening(self, self_)
        }

        fn address_family(
            &mut self,
            self_: Resource<TcpSocket>,
        ) -> wasmtime::Result<IpAddressFamily> {
            AsyncHostTcpSocket::address_family(self, self_)
        }

        fn set_listen_backlog_size(
            &mut self,
            self_: Resource<TcpSocket>,
            value: u64,
        ) -> Result<(), SocketError> {
            AsyncHostTcpSocket::set_listen_backlog_size(self, self_, value)
        }

        fn keep_alive_enabled(&mut self, self_: Resource<TcpSocket>) -> Result<bool, SocketError> {
            AsyncHostTcpSocket::keep_alive_enabled(self, self_)
        }

        fn set_keep_alive_enabled(
            &mut self,
            self_: Resource<TcpSocket>,
            value: bool,
        ) -> Result<(), SocketError> {
            AsyncHostTcpSocket::set_keep_alive_enabled(self, self_, value)
        }

        fn keep_alive_idle_time(
            &mut self,
            self_: Resource<TcpSocket>,
        ) -> Result<Duration, SocketError> {
            AsyncHostTcpSocket::keep_alive_idle_time(self, self_)
        }

        fn set_keep_alive_idle_time(
            &mut self,
            self_: Resource<TcpSocket>,
            value: Duration,
        ) -> Result<(), SocketError> {
            AsyncHostTcpSocket::set_keep_alive_idle_time(self, self_, value)
        }

        fn keep_alive_interval(
            &mut self,
            self_: Resource<TcpSocket>,
        ) -> Result<Duration, SocketError> {
            AsyncHostTcpSocket::keep_alive_interval(self, self_)
        }

        fn set_keep_alive_interval(
            &mut self,
            self_: Resource<TcpSocket>,
            value: Duration,
        ) -> Result<(), SocketError> {
            AsyncHostTcpSocket::set_keep_alive_interval(self, self_, value)
        }

        fn keep_alive_count(&mut self, self_: Resource<TcpSocket>) -> Result<u32, SocketError> {
            AsyncHostTcpSocket::keep_alive_count(self, self_)
        }

        fn set_keep_alive_count(
            &mut self,
            self_: Resource<TcpSocket>,
            value: u32,
        ) -> Result<(), SocketError> {
            AsyncHostTcpSocket::set_keep_alive_count(self, self_, value)
        }

        fn hop_limit(&mut self, self_: Resource<TcpSocket>) -> Result<u8, SocketError> {
            AsyncHostTcpSocket::hop_limit(self, self_)
        }

        fn set_hop_limit(
            &mut self,
            self_: Resource<TcpSocket>,
            value: u8,
        ) -> Result<(), SocketError> {
            AsyncHostTcpSocket::set_hop_limit(self, self_, value)
        }

        fn receive_buffer_size(&mut self, self_: Resource<TcpSocket>) -> Result<u64, SocketError> {
            AsyncHostTcpSocket::receive_buffer_size(self, self_)
        }

        fn set_receive_buffer_size(
            &mut self,
            self_: Resource<TcpSocket>,
            value: u64,
        ) -> Result<(), SocketError> {
            AsyncHostTcpSocket::set_receive_buffer_size(self, self_, value)
        }

        fn send_buffer_size(&mut self, self_: Resource<TcpSocket>) -> Result<u64, SocketError> {
            AsyncHostTcpSocket::send_buffer_size(self, self_)
        }

        fn set_send_buffer_size(
            &mut self,
            self_: Resource<TcpSocket>,
            value: u64,
        ) -> Result<(), SocketError> {
            AsyncHostTcpSocket::set_send_buffer_size(self, self_, value)
        }

        fn subscribe(
            &mut self,
            self_: Resource<TcpSocket>,
        ) -> wasmtime::Result<Resource<Pollable>> {
            AsyncHostTcpSocket::subscribe(self, self_)
        }

        fn shutdown(
            &mut self,
            self_: Resource<TcpSocket>,
            shutdown_type: ShutdownType,
        ) -> Result<(), SocketError> {
            AsyncHostTcpSocket::shutdown(self, self_, shutdown_type.into())
        }

        fn drop(&mut self, rep: Resource<TcpSocket>) -> wasmtime::Result<()> {
            AsyncHostTcpSocket::drop(self, rep)
        }
    }

    impl From<ShutdownType> for async_tcp::ShutdownType {
        fn from(other: ShutdownType) -> Self {
            match other {
                ShutdownType::Receive => async_tcp::ShutdownType::Receive,
                ShutdownType::Send => async_tcp::ShutdownType::Send,
                ShutdownType::Both => async_tcp::ShutdownType::Both,
            }
        }
    }
}
