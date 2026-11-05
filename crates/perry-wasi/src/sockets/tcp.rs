//! Native TCP state machine used by the WASI socket adapters.
//!
//! Address checks run on the caller. Only owned sockets, addresses, options and
//! byte buffers cross to the context's Turnloop thread.
use super::{SocketAddrUse, SocketPolicy, WasiSocketsCtx};
use crate::p2::{bindings::sockets::network::ErrorCode, SocketError, SocketResult};
use perry_component_io::{Resource, ResourceRequest, Submitter};
use socket2::{Domain, Protocol, Socket, Type};
use std::{net::SocketAddr, time::Duration};
use turnloop::{backend::Platform, AsyncIo, SocketOption, SocketOptionKind};

mod native;
pub use native::Failure;
pub(super) mod options;
#[cfg(test)]
mod tests;

/// Address family is fixed at creation; IPv6 sockets always use IPV6_V6ONLY.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Ipv4,
    Ipv6,
}
impl Family {
    fn matches(self, address: SocketAddr) -> bool {
        matches!(
            (self, address),
            (Self::Ipv4, SocketAddr::V4(_)) | (Self::Ipv6, SocketAddr::V6(_))
        )
    }
    pub(crate) fn validate(self, address: SocketAddr, remote: bool) -> SocketResult<()> {
        let ip = address.ip();
        if !self.matches(address)
            || ip.is_multicast()
            || matches!(ip, std::net::IpAddr::V6(ip) if ip.to_ipv4_mapped().is_some())
            || (remote
                && (address.port() == 0
                    || ip.is_unspecified()
                    || matches!(ip, std::net::IpAddr::V4(ip) if ip.is_broadcast())))
        {
            return Err(ErrorCode::InvalidArgument.into());
        }
        Ok(())
    }
    pub(crate) fn unspecified(self) -> SocketAddr {
        match self {
            Self::Ipv4 => SocketAddr::from(([0, 0, 0, 0], 0)),
            Self::Ipv6 => SocketAddr::from(([0u16; 8], 0)),
        }
    }
}

struct NativeTcp {
    stream: AsyncIo<Platform>,
    reading: bool,
    writing: bool,
    write_cancelled: bool,
    inherited: Vec<SocketOption>,
}
impl NativeTcp {
    fn new(stream: AsyncIo<Platform>) -> Self {
        Self {
            stream,
            reading: false,
            writing: false,
            write_cancelled: false,
            inherited: Vec::new(),
        }
    }
}
enum State {
    Fresh {
        socket: Socket,
        bound: bool,
    },
    // A duplicate handle to the same OS socket exposes synchronous controls
    // while the original handle is owned by an in-flight P3 connect operation.
    #[cfg(feature = "p3")]
    Connecting(Socket),
    Listening(Resource<NativeTcp>),
    Connected {
        socket: Resource<NativeTcp>,
        local: SocketAddr,
        peer: SocketAddr,
    },
    Closed,
}

/// A socket owns one native transport throughout binding, connecting and I/O.
/// Dropping a pending operation or this owner releases that transport through
/// Turnloop. Accepted connections have independent native resource lifetimes.
pub struct TcpSocket {
    state: State,
    family: Family,
    submitter: Submitter,
    policy: SocketPolicy,
    backlog: u32,
    // Only explicitly changed settings need repair on platforms where accept
    // doesn't inherit them. At most one entry per supported option is retained.
    inherited: Vec<SocketOption>,
}
impl TcpSocket {
    pub fn new(ctx: &WasiSocketsCtx, family: Family) -> SocketResult<Self> {
        if !ctx.tcp.allowed {
            return Err(ErrorCode::AccessDenied.into());
        }
        let socket = Socket::new(
            match family {
                Family::Ipv4 => Domain::IPV4,
                Family::Ipv6 => Domain::IPV6,
            },
            Type::STREAM,
            Some(Protocol::TCP),
        )?;
        socket.set_nonblocking(true)?;
        if family == Family::Ipv6 {
            socket.set_only_v6(true)?;
        }
        Ok(Self {
            state: State::Fresh {
                socket,
                bound: false,
            },
            family,
            submitter: ctx.submitter.clone(),
            policy: ctx.tcp.clone(),
            backlog: 128,
            inherited: Vec::new(),
        })
    }

    pub fn family(&self) -> Family {
        self.family
    }
    pub fn is_bound(&self) -> bool {
        matches!(self.state, State::Fresh { bound: true, .. })
    }
    pub fn is_listening(&self) -> bool {
        matches!(self.state, State::Listening(_))
    }

    pub async fn bind(&mut self, address: SocketAddr) -> SocketResult<()> {
        if !matches!(self.state, State::Fresh { bound: false, .. }) {
            return Err(ErrorCode::InvalidState.into());
        }
        self.family.validate(address, false)?;
        self.policy.check(address, SocketAddrUse::TcpBind).await?;
        let State::Fresh { socket, bound } = &mut self.state else {
            unreachable!()
        };
        // POSIX permits reusing TIME_WAIT endpoints. On Windows SO_REUSEADDR
        // permits stealing live bindings, so it must remain disabled there.
        #[cfg(unix)]
        socket.set_reuse_address(true)?;
        socket.bind(&address.into())?;
        *bound = true;
        Ok(())
    }

    pub(crate) fn check_connect(&self, address: SocketAddr) -> SocketResult<()> {
        if !matches!(self.state, State::Fresh { .. }) {
            return Err(ErrorCode::InvalidState.into());
        }
        self.family.validate(address, true)
    }
    #[cfg(feature = "p3")]
    pub(crate) fn connecting_view(&self) -> SocketResult<Self> {
        let State::Fresh { socket, .. } = &self.state else {
            return Err(ErrorCode::InvalidState.into());
        };
        Ok(Self {
            state: State::Connecting(socket.try_clone()?),
            family: self.family,
            submitter: self.submitter.clone(),
            policy: self.policy.clone(),
            backlog: self.backlog,
            inherited: Vec::new(),
        })
    }
    pub(crate) fn connection(&self) -> SocketResult<TcpConnection> {
        match &self.state {
            State::Connected { socket, .. } => Ok(TcpConnection(socket.clone())),
            _ => Err(ErrorCode::InvalidState.into()),
        }
    }

    pub async fn connect(&mut self, address: SocketAddr) -> SocketResult<()> {
        if !matches!(self.state, State::Fresh { .. }) {
            return Err(ErrorCode::InvalidState.into());
        }
        self.family.validate(address, true)?;
        self.policy
            .check(address, SocketAddrUse::TcpConnect)
            .await?;
        // connect_bound preserves both an explicit local port and options
        // configured before the handshake. Implicit binding is also checked.
        if !self.is_bound() {
            self.bind(self.family.unspecified()).await?;
        }
        let State::Fresh { socket, .. } = std::mem::replace(&mut self.state, State::Closed) else {
            unreachable!()
        };
        let task = self
            .submitter
            .register(move |io| async move {
                let transport = native::detach(socket, false)?;
                let stream = io
                    .connect_bound(transport, address, turnloop::TcpOpts::default())
                    .await
                    .map_err(native::Failure::from)?;
                Ok::<_, native::Failure>(NativeTcp::new(stream))
            })
            .map_err(SocketError::trap)?;
        let socket = task
            .await
            .map_err(SocketError::trap)?
            .map_err(native::Failure::socket)?;
        let local = native::local(&socket)?;
        self.state = State::Connected {
            socket,
            local,
            peer: address,
        };
        Ok(())
    }

    /// WASI 0.2 listeners must have been explicitly bound.
    pub async fn listen(&mut self) -> SocketResult<()> {
        if !self.is_bound() {
            return Err(ErrorCode::InvalidState.into());
        }
        self.policy
            .check(self.local_address()?, SocketAddrUse::TcpListen)
            .await?;
        let State::Fresh { socket, .. } = &self.state else {
            unreachable!()
        };
        socket.listen(self.backlog as i32)?;
        let State::Fresh { socket, .. } = std::mem::replace(&mut self.state, State::Closed) else {
            unreachable!()
        };
        let inherited = self.inherited.clone();
        let task = self
            .submitter
            .register(move |io| async move {
                let transport = native::detach(socket, true)?;
                let handle = io
                    .driver()
                    .attach(transport, turnloop::Token(0))
                    .map_err(native::Failure::from)?;
                let mut native = NativeTcp::new(io.io(handle));
                native.inherited = inherited;
                Ok::<_, native::Failure>(native)
            })
            .map_err(SocketError::trap)?;
        self.state = State::Listening(
            task.await
                .map_err(SocketError::trap)?
                .map_err(native::Failure::socket)?,
        );
        Ok(())
    }

    /// Own the listener reference while acceptance is pending. The future may
    /// outlive this wrapper; address callbacks still execute on its caller.
    pub fn accept(&self) -> impl std::future::Future<Output = SocketResult<Self>> + Send + 'static {
        let listener = match &self.state {
            State::Listening(listener) => Some(listener.clone()),
            _ => None,
        };
        let family = self.family;
        let submitter = self.submitter.clone();
        let policy = self.policy.clone();
        async move {
            let listener = listener.ok_or(ErrorCode::InvalidState)?;
            loop {
                let task = listener
                    .derive(move |io, local| async move {
                        let handle = local.borrow_mut().stream.handle();
                        let stream = io.accept(handle).await.map_err(native::Failure::from)?;
                        // Read current overrides after acceptance: settings changed
                        // while this operation was pending must not be overwritten
                        // by a stale snapshot captured when accept started.
                        let options = local.borrow_mut().inherited.clone();
                        drop(local);
                        for option in options {
                            io.driver()
                                .set_option(stream.handle(), option)
                                .map_err(native::Failure::from)?;
                        }
                        Ok::<_, native::Failure>(NativeTcp::new(stream))
                    })
                    .map_err(SocketError::trap)?;
                let socket = task
                    .await
                    .map_err(SocketError::trap)?
                    .map_err(native::Failure::socket)?;
                let peer = native::peer(&socket)?;
                if policy.check(peer, SocketAddrUse::TcpAccept).await.is_err() {
                    drop(socket);
                    continue;
                }
                let local = native::local(&socket)?;
                return Ok(Self {
                    state: State::Connected {
                        socket,
                        local,
                        peer,
                    },
                    family,
                    submitter,
                    policy,
                    backlog: 128,
                    inherited: Vec::new(),
                });
            }
        }
    }

    pub fn local_address(&self) -> SocketResult<SocketAddr> {
        match &self.state {
            #[cfg(feature = "p3")]
            State::Connecting(socket) => socket
                .local_addr()?
                .as_socket()
                .filter(|address| address.port() != 0)
                .ok_or_else(|| ErrorCode::InvalidState.into()),
            State::Fresh {
                socket,
                bound: true,
            } => socket
                .local_addr()?
                .as_socket()
                .ok_or_else(|| ErrorCode::InvalidState.into()),
            State::Listening(socket) => native::local(socket),
            State::Connected { local, .. } => Ok(*local),
            _ => Err(ErrorCode::InvalidState.into()),
        }
    }
    pub fn remote_address(&self) -> SocketResult<SocketAddr> {
        match self.state {
            State::Connected { peer, .. } => Ok(peer),
            _ => Err(ErrorCode::InvalidState.into()),
        }
    }

    /// Begin a bounded owned read. The WASI adapter serializes reads and keeps
    /// the request through readiness; cancellation of a request alone does not
    /// discard Turnloop's pending read slot on a still-live socket.
    pub fn read(&self, size: usize) -> SocketResult<ResourceRequest<Result<Vec<u8>, Failure>>> {
        let State::Connected { socket, .. } = &self.state else {
            return Err(ErrorCode::InvalidState.into());
        };
        native::read(socket, size).map_err(SocketError::trap)
    }
    /// Begin one owned write, including flush to the native transport.
    pub fn write(&self, bytes: Vec<u8>) -> SocketResult<ResourceRequest<Result<(), Failure>>> {
        let State::Connected { socket, .. } = &self.state else {
            return Err(ErrorCode::InvalidState.into());
        };
        if bytes.len() > crate::MAX_READ_SIZE_ALLOC {
            return Err(ErrorCode::InvalidArgument.into());
        }
        native::write(socket, bytes).map_err(SocketError::trap)
    }
    /// Deliver FIN after previously accepted writes while preserving reads.
    pub async fn shutdown_send(&self) -> SocketResult<()> {
        let State::Connected { socket, .. } = &self.state else {
            return Err(ErrorCode::InvalidState.into());
        };
        let task = socket
            .call(|_, local| async move {
                if local.borrow_mut().writing {
                    return Err(native::Failure::Socket(ErrorCode::ConcurrencyConflict));
                }
                std::future::poll_fn(|cx| local.borrow_mut().stream.poll_shutdown(cx))
                    .await
                    .map_err(native::Failure::from)
            })
            .map_err(SocketError::trap)?;
        task.await
            .map_err(SocketError::trap)?
            .map_err(native::Failure::socket)
    }

    /// Stop receiving and discard the adapter's native pending/buffered input.
    /// The stream adapter first cancels and awaits its owned read request so no
    /// other future is polling that direction when its completion is discarded.
    pub async fn shutdown_receive(&self) -> SocketResult<()> {
        let State::Connected { socket, .. } = &self.state else {
            return Err(ErrorCode::InvalidState.into());
        };
        let task = socket
            .call(|_, local| async move {
                let mut native = local.borrow_mut();
                if native.reading {
                    return Err(native::Failure::Socket(ErrorCode::ConcurrencyConflict));
                }
                native
                    .stream
                    .shutdown_receive()
                    .map_err(native::Failure::from)
            })
            .map_err(SocketError::trap)?;
        task.await
            .map_err(SocketError::trap)?
            .map_err(native::Failure::socket)
    }

    fn get_option(&self, kind: SocketOptionKind) -> SocketResult<SocketOption> {
        match &self.state {
            #[cfg(feature = "p3")]
            State::Connecting(socket) => {
                options::get(socket, self.family, kind).map_err(Into::into)
            }
            State::Fresh { socket, .. } => {
                options::get(socket, self.family, kind).map_err(Into::into)
            }
            State::Listening(socket) | State::Connected { socket, .. } => {
                native::get_option(socket, kind)
            }
            State::Closed => Err(ErrorCode::InvalidState.into()),
        }
    }
    fn set_option(&mut self, option: SocketOption) -> SocketResult<()> {
        match &self.state {
            #[cfg(feature = "p3")]
            State::Connecting(socket) => options::set(socket, self.family, option)?,
            State::Fresh { socket, .. } => options::set(socket, self.family, option)?,
            State::Listening(socket) | State::Connected { socket, .. } => {
                native::set_option(socket, option)?
            }
            State::Closed => return Err(ErrorCode::InvalidState.into()),
        }
        // Each probe-schedule field is recorded separately: a partial update
        // must not erase earlier explicit values required for accept inheritance.
        remember_option(&mut self.inherited, option);
        Ok(())
    }
    pub fn set_listen_backlog_size(&mut self, size: u64) -> SocketResult<()> {
        if size == 0 {
            return Err(ErrorCode::InvalidArgument.into());
        }
        let size = size.min(i32::MAX as u64) as u32;
        match &self.state {
            State::Fresh { .. } => (),
            State::Listening(socket) => {
                native::set_option(socket, SocketOption::ListenBacklog(size))?
            }
            _ => return Err(ErrorCode::InvalidState.into()),
        }
        self.backlog = size;
        Ok(())
    }
    pub fn keep_alive_enabled(&self) -> SocketResult<bool> {
        match self.get_option(SocketOptionKind::KeepAliveEnabled)? {
            SocketOption::KeepAliveEnabled(v) => Ok(v),
            _ => unreachable!(),
        }
    }
    pub fn set_keep_alive_enabled(&mut self, value: bool) -> SocketResult<()> {
        self.set_option(SocketOption::KeepAliveEnabled(value))
    }
    fn keep_alive(&self) -> SocketResult<turnloop::KeepAlive> {
        match self.get_option(SocketOptionKind::KeepAliveSettings)? {
            SocketOption::KeepAliveSettings(v) => Ok(v),
            _ => unreachable!(),
        }
    }
    pub fn keep_alive_idle_time(&self) -> SocketResult<u64> {
        Ok(self
            .keep_alive()?
            .idle
            .ok_or(ErrorCode::NotSupported)?
            .as_nanos()
            .min(u64::MAX as u128) as u64)
    }
    pub fn keep_alive_interval(&self) -> SocketResult<u64> {
        Ok(self
            .keep_alive()?
            .interval
            .ok_or(ErrorCode::NotSupported)?
            .as_nanos()
            .min(u64::MAX as u128) as u64)
    }
    pub fn keep_alive_count(&self) -> SocketResult<u32> {
        self.keep_alive()?
            .count
            .ok_or_else(|| ErrorCode::NotSupported.into())
    }
    pub fn set_keep_alive_idle_time(&mut self, value: u64) -> SocketResult<()> {
        self.set_option(SocketOption::KeepAliveSettings(turnloop::KeepAlive {
            idle: Some(probe_time(value)?),
            ..Default::default()
        }))
    }
    pub fn set_keep_alive_interval(&mut self, value: u64) -> SocketResult<()> {
        self.set_option(SocketOption::KeepAliveSettings(turnloop::KeepAlive {
            interval: Some(probe_time(value)?),
            ..Default::default()
        }))
    }
    pub fn set_keep_alive_count(&mut self, value: u32) -> SocketResult<()> {
        if value == 0 {
            return Err(ErrorCode::InvalidArgument.into());
        }
        self.set_option(SocketOption::KeepAliveSettings(turnloop::KeepAlive {
            count: Some(value.min(127)),
            ..Default::default()
        }))
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
            SocketOption::RecvBufferSize(v) => Ok(reported_buffer_size(v)),
            _ => unreachable!(),
        }
    }
    pub fn set_receive_buffer_size(&mut self, value: u64) -> SocketResult<()> {
        self.set_option(SocketOption::RecvBufferSize(buffer_size(value)?))
    }
    pub fn send_buffer_size(&self) -> SocketResult<u64> {
        match self.get_option(SocketOptionKind::SendBufferSize)? {
            SocketOption::SendBufferSize(v) => Ok(reported_buffer_size(v)),
            _ => unreachable!(),
        }
    }
    pub fn set_send_buffer_size(&mut self, value: u64) -> SocketResult<()> {
        self.set_option(SocketOption::SendBufferSize(buffer_size(value)?))
    }
}
fn probe_time(nanos: u64) -> SocketResult<Duration> {
    if nanos == 0 {
        return Err(ErrorCode::InvalidArgument.into());
    }
    // Native options have one-second precision. Normalize before both the
    // fresh-socket setter and Turnloop so accept cannot change a rounded value.
    Ok(Duration::from_secs(
        nanos.div_ceil(1_000_000_000).clamp(1, i16::MAX as u64),
    ))
}
pub(super) fn reported_buffer_size(size: u32) -> u64 {
    // Linux includes bookkeeping overhead in SO_RCVBUF/SO_SNDBUF. Match the
    // WASI provider convention of reporting application buffer bytes.
    if cfg!(target_os = "linux") {
        u64::from(size / 2)
    } else {
        u64::from(size)
    }
}
pub(super) fn buffer_size(size: u64) -> SocketResult<u32> {
    if size == 0 {
        return Err(ErrorCode::InvalidArgument.into());
    }
    Ok(size.min(i32::MAX as u64) as u32)
}

fn remember_option(options: &mut Vec<SocketOption>, option: SocketOption) {
    // Backlog belongs to the listener; it is never an accepted socket option.
    if matches!(option, SocketOption::ListenBacklog(_)) {
        return;
    }
    if let Some(existing) = options
        .iter_mut()
        .find(|v| options::same_setting(**v, option))
    {
        *existing = option;
    } else {
        options.push(option);
    }
}

/// Stream endpoints retain the transport independently of the socket wrapper.
#[derive(Clone)]
pub(crate) struct TcpConnection(Resource<NativeTcp>);
impl TcpConnection {
    pub(crate) fn read(
        &self,
        size: usize,
    ) -> Result<ResourceRequest<Result<Vec<u8>, Failure>>, perry_component_io::Error> {
        native::read(&self.0, size)
    }
    pub(crate) fn write(
        &self,
        bytes: Vec<u8>,
    ) -> Result<ResourceRequest<Result<(), Failure>>, perry_component_io::Error> {
        native::write(&self.0, bytes)
    }
    pub(crate) fn close_receive(
        &self,
        pending: Option<ResourceRequest<Result<Vec<u8>, Failure>>>,
    ) -> Result<ResourceRequest<Result<(), Failure>>, perry_component_io::Error> {
        if let Some(pending) = &pending {
            pending.cancel();
        }
        self.0.call_cleanup(move |_, local| async move {
            // Native requests contain only owned native bytes. Waiting here
            // keeps cancellation and its acknowledgement on the same driver,
            // without moving any guest state or callback to that thread.
            if let Some(pending) = pending {
                match pending.await {
                    Ok(_) | Err(perry_component_io::Error::Cancelled) => (),
                    Err(error) => return Err(Failure::Driver(error)),
                }
            }
            let mut native = local.borrow_mut();
            native.stream.shutdown_receive().map_err(Failure::from)
        })
    }
    pub(crate) fn close_send(
        &self,
        pending: Option<ResourceRequest<Result<(), Failure>>>,
    ) -> Result<ResourceRequest<Result<(), Failure>>, perry_component_io::Error> {
        self.0.call_cleanup(move |_, local| async move {
            if let Some(pending) = pending {
                pending.await.map_err(Failure::Driver)??;
            }
            std::future::poll_fn(|cx| local.borrow_mut().stream.poll_shutdown(cx))
                .await
                .map_err(Failure::from)
        })
    }
}
