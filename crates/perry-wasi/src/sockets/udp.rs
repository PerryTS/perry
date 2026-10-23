//! UDP owners and address capabilities. Native packet operations stay on the
//! Turnloop thread; permission futures stay on the calling agent.
use super::{
    tcp::{self, Failure, Family},
    SocketAddrUse, SocketPolicy, WasiSocketsCtx,
};
use crate::p2::{bindings::sockets::network::ErrorCode, SocketError, SocketResult};
use perry_component_io::{Resource, ResourceRequest, Submitter};
use socket2::{Domain, Protocol, Socket, Type};
use std::net::SocketAddr;
use turnloop::{backend::Platform, AsyncDatagram, SocketOption, SocketOptionKind};
mod native;
#[cfg(test)]
mod tests;

pub(crate) struct NativeUdp {
    io: AsyncDatagram<Platform>,
    reading: bool,
    writing: bool,
}
enum State {
    Fresh(Socket),
    Bound(Resource<NativeUdp>),
    Closed,
}
pub struct UdpSocket {
    state: State,
    family: Family,
    submitter: Submitter,
    policy: SocketPolicy,
    remote: Option<SocketAddr>,
}
impl UdpSocket {
    pub fn new(ctx: &WasiSocketsCtx, family: Family) -> SocketResult<Self> {
        if !ctx.udp.allowed {
            return Err(ErrorCode::AccessDenied.into());
        }
        let socket = Socket::new(
            if family == Family::Ipv4 {
                Domain::IPV4
            } else {
                Domain::IPV6
            },
            Type::DGRAM,
            Some(Protocol::UDP),
        )?;
        socket.set_nonblocking(true)?;
        if family == Family::Ipv6 {
            socket.set_only_v6(true)?;
        }
        Ok(Self {
            state: State::Fresh(socket),
            family,
            submitter: ctx.submitter.clone(),
            policy: ctx.udp.clone(),
            remote: None,
        })
    }
    pub fn family(&self) -> Family {
        self.family
    }
    pub fn is_bound(&self) -> bool {
        matches!(self.state, State::Bound(_))
    }
    pub async fn bind(&mut self, address: SocketAddr) -> SocketResult<()> {
        if !matches!(self.state, State::Fresh(_)) {
            return Err(ErrorCode::InvalidState.into());
        }
        self.family.validate(address, false)?;
        self.policy.check(address, SocketAddrUse::UdpBind).await?;
        let State::Fresh(socket) = &self.state else {
            unreachable!()
        };
        // Keep the OS default exclusive binding. UDP has no TIME_WAIT reuse need.
        socket.bind(&address.into())?;
        let State::Fresh(socket) = std::mem::replace(&mut self.state, State::Closed) else {
            unreachable!()
        };
        let task = self
            .submitter
            .register(move |io| async move {
                #[cfg(unix)]
                let transport = turnloop::Detached::from_fd(socket.into());
                #[cfg(windows)]
                let transport = turnloop::Detached::from_socket(socket.into());
                let handle = io
                    .driver()
                    .attach(transport.map_err(Failure::from)?, turnloop::Token(0))
                    .map_err(Failure::from)?;
                Ok::<_, Failure>(NativeUdp {
                    io: io.datagram(handle),
                    reading: false,
                    writing: false,
                })
            })
            .map_err(SocketError::trap)?;
        self.state = State::Bound(
            task.await
                .map_err(SocketError::trap)?
                .map_err(Failure::socket)?,
        );
        Ok(())
    }
    fn bound(&self) -> SocketResult<&Resource<NativeUdp>> {
        match &self.state {
            State::Bound(socket) => Ok(socket),
            _ => Err(ErrorCode::InvalidState.into()),
        }
    }
    pub fn local_address(&self) -> SocketResult<SocketAddr> {
        let task = self
            .bound()?
            .call(|io, local| async move {
                io.driver()
                    .local_addr(local.borrow_mut().io.handle())
                    .map_err(Failure::from)
            })
            .map_err(SocketError::trap)?;
        futures_lite::future::block_on(task)
            .map_err(SocketError::trap)?
            .map_err(Failure::socket)
    }
    pub fn remote_address(&self) -> SocketResult<SocketAddr> {
        self.remote.ok_or_else(|| ErrorCode::InvalidState.into())
    }
    /// Associate streams without replacing the bound transport. Peer restriction
    /// is implemented by receive filtering and explicit send destinations, so
    /// reassociation keeps the bound port and configured options. This does not
    /// perform a native UDP connect or change a wildcard local address.
    pub async fn stream(&mut self, remote: Option<SocketAddr>) -> SocketResult<UdpConnection> {
        self.bound()?;
        if let Some(remote) = remote {
            self.family.validate(remote, true)?;
            self.policy.check(remote, SocketAddrUse::UdpConnect).await?;
        }
        let socket = self.bound()?.clone();
        self.remote = remote;
        Ok(UdpConnection {
            socket,
            family: self.family,
            policy: self.policy.clone(),
            remote,
        })
    }
    /// P3 association permits implicit binding. Permission checks happen before
    /// binding, and packet filtering remains in the version-specific adapter.
    #[cfg(feature = "p3")]
    pub(crate) async fn connect(&mut self, remote: SocketAddr) -> SocketResult<()> {
        self.family.validate(remote, true)?;
        self.policy.check(remote, SocketAddrUse::UdpConnect).await?;
        if !self.is_bound() {
            self.bind(self.family.unspecified()).await?;
        }
        self.bound()?;
        self.remote = Some(remote);
        Ok(())
    }
    #[cfg(feature = "p3")]
    pub(crate) fn disconnect(&mut self) -> SocketResult<()> {
        self.bound()?;
        if self.remote.take().is_none() {
            return Err(ErrorCode::InvalidState.into());
        }
        Ok(())
    }
    #[cfg(feature = "p3")]
    pub(crate) fn unfiltered_connection(&self) -> SocketResult<UdpConnection> {
        Ok(UdpConnection {
            socket: self.bound()?.clone(),
            family: self.family,
            policy: self.policy.clone(),
            remote: None,
        })
    }
    /// Validate and authorize a destination before an implicit bind or send.
    #[cfg(feature = "p3")]
    pub(crate) async fn prepare_send(
        &mut self,
        data: Vec<u8>,
        destination: Option<SocketAddr>,
    ) -> SocketResult<(UdpConnection, SendTask)> {
        let destination = match (self.remote, destination) {
            (None, None) => return Err(ErrorCode::InvalidArgument.into()),
            (Some(expected), Some(actual)) if expected != actual => {
                return Err(ErrorCode::InvalidArgument.into())
            }
            (Some(remote), _) | (None, Some(remote)) => remote,
        };
        self.family.validate(destination, true)?;
        let maximum = if self.family == Family::Ipv4 {
            65_507
        } else {
            65_527
        };
        if data.len() > maximum {
            return Err(ErrorCode::DatagramTooLarge.into());
        }
        self.policy
            .check(destination, SocketAddrUse::UdpOutgoingDatagram)
            .await?;
        if !self.is_bound() {
            self.bind(self.family.unspecified()).await?;
        }
        let connection = self.unfiltered_connection()?;
        let task = native::send(self.bound()?, data, destination).map_err(SocketError::trap)?;
        Ok((connection, task))
    }
    fn get_option(&self, kind: SocketOptionKind) -> SocketResult<SocketOption> {
        match &self.state {
            State::Fresh(socket) => Ok(tcp::options::get(socket, self.family, kind)?),
            State::Bound(socket) => {
                let task = socket
                    .call(move |io, local| async move {
                        io.driver()
                            .get_option(local.borrow_mut().io.handle(), kind)
                            .map_err(Failure::from)
                    })
                    .map_err(SocketError::trap)?;
                futures_lite::future::block_on(task)
                    .map_err(SocketError::trap)?
                    .map_err(Failure::socket)
            }
            State::Closed => Err(ErrorCode::InvalidState.into()),
        }
    }
    fn set_option(&mut self, option: SocketOption) -> SocketResult<()> {
        match &self.state {
            State::Fresh(socket) => Ok(tcp::options::set(socket, self.family, option)?),
            State::Bound(socket) => {
                let task = socket
                    .call(move |io, local| async move {
                        io.driver()
                            .set_option(local.borrow_mut().io.handle(), option)
                            .map_err(Failure::from)
                    })
                    .map_err(SocketError::trap)?;
                futures_lite::future::block_on(task)
                    .map_err(SocketError::trap)?
                    .map_err(Failure::socket)
            }
            State::Closed => Err(ErrorCode::InvalidState.into()),
        }
    }
    pub fn hop_limit(&self) -> SocketResult<u8> {
        match self.get_option(SocketOptionKind::Ttl)? {
            SocketOption::Ttl(v) => u8::try_from(v).map_err(|_| ErrorCode::Unknown.into()),
            _ => unreachable!(),
        }
    }
    pub fn set_hop_limit(&mut self, value: u8) -> SocketResult<()> {
        if value == 0 {
            return Err(ErrorCode::InvalidArgument.into());
        }
        self.set_option(SocketOption::Ttl(value.into()))
    }
    pub fn receive_buffer_size(&self) -> SocketResult<u64> {
        match self.get_option(SocketOptionKind::RecvBufferSize)? {
            SocketOption::RecvBufferSize(v) => Ok(tcp::reported_buffer_size(v)),
            _ => unreachable!(),
        }
    }
    pub fn set_receive_buffer_size(&mut self, value: u64) -> SocketResult<()> {
        self.set_option(SocketOption::RecvBufferSize(tcp::buffer_size(value)?))
    }
    pub fn send_buffer_size(&self) -> SocketResult<u64> {
        match self.get_option(SocketOptionKind::SendBufferSize)? {
            SocketOption::SendBufferSize(v) => Ok(tcp::reported_buffer_size(v)),
            _ => unreachable!(),
        }
    }
    pub fn set_send_buffer_size(&mut self, value: u64) -> SocketResult<()> {
        self.set_option(SocketOption::SendBufferSize(tcp::buffer_size(value)?))
    }
}

#[derive(Debug)]
pub struct Datagram {
    pub data: Vec<u8>,
    pub from: SocketAddr,
}
pub type ReceiveTask = ResourceRequest<Result<Datagram, Failure>>;
pub type SendTask = ResourceRequest<Result<(), Failure>>;
#[derive(Clone)]
pub struct UdpConnection {
    socket: Resource<NativeUdp>,
    family: Family,
    policy: SocketPolicy,
    remote: Option<SocketAddr>,
}
impl UdpConnection {
    pub fn receive(&self) -> Result<ReceiveTask, perry_component_io::Error> {
        native::receive(&self.socket, self.remote)
    }
    pub async fn send(
        &self,
        data: Vec<u8>,
        destination: Option<SocketAddr>,
    ) -> SocketResult<SendTask> {
        let destination = match (self.remote, destination) {
            (None, None) => return Err(ErrorCode::InvalidArgument.into()),
            (Some(expected), Some(actual)) if expected != actual => {
                return Err(ErrorCode::InvalidArgument.into())
            }
            (Some(remote), _) | (None, Some(remote)) => remote,
        };
        self.family.validate(destination, true)?;
        // Ordinary UDP payload limits, excluding optional IPv6 jumbograms.
        let maximum = if self.family == Family::Ipv4 {
            65_507
        } else {
            65_527
        };
        if data.len() > maximum {
            return Err(ErrorCode::DatagramTooLarge.into());
        }
        self.policy
            .check(destination, SocketAddrUse::UdpOutgoingDatagram)
            .await?;
        native::send(&self.socket, data, destination).map_err(SocketError::trap)
    }
    pub fn cancel_receive(
        &self,
        pending: Option<ReceiveTask>,
    ) -> Result<SendTask, perry_component_io::Error> {
        if let Some(pending) = &pending {
            pending.cancel();
        }
        self.socket.call_cleanup(move |_, local| async move {
            if let Some(pending) = pending {
                match pending.await {
                    Ok(_) | Err(perry_component_io::Error::Cancelled) => (),
                    Err(error) => return Err(Failure::Driver(error)),
                }
            }
            local.borrow_mut().io.cancel_receive();
            Ok(())
        })
    }
    pub fn finish_send(
        &self,
        pending: Option<SendTask>,
    ) -> Result<SendTask, perry_component_io::Error> {
        self.socket.call_cleanup(move |_, local| async move {
            if let Some(pending) = pending {
                pending.await.map_err(Failure::Driver)??;
            }
            std::future::poll_fn(|cx| local.borrow_mut().io.poll_flush(cx))
                .await
                .map_err(Failure::from)
        })
    }
}
