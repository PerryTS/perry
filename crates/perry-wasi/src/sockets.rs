//! Networking capabilities and owned native operations for WASI providers.
use perry_component_io::Submitter;
use std::{future::Future, net::SocketAddr, pin::Pin, sync::Arc};
use wasmtime::component::{HasData, ResourceTable};

pub mod tcp;
pub mod udp;

/// The operation for which an address capability is requested.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketAddrUse {
    TcpBind,
    TcpListen,
    TcpConnect,
    TcpAccept,
    UdpBind,
    UdpConnect,
    UdpOutgoingDatagram,
}

type AddressCheck = Arc<
    dyn Fn(SocketAddr, SocketAddrUse) -> Pin<Box<dyn Future<Output = bool> + Send>> + Send + Sync,
>;

#[derive(Clone)]
struct SocketPolicy {
    allowed: bool,
    check: AddressCheck,
}
impl SocketPolicy {
    async fn check(
        &self,
        address: SocketAddr,
        usage: SocketAddrUse,
    ) -> crate::p2::SocketResult<()> {
        if self.allowed && (self.check)(address, usage).await {
            Ok(())
        } else {
            Err(crate::p2::bindings::sockets::network::ErrorCode::AccessDenied.into())
        }
    }
}

pub struct WasiSockets;
impl HasData for WasiSockets {
    type Data<'a> = WasiSocketsCtxView<'a>;
}

pub struct WasiSocketsCtx {
    pub(crate) submitter: Submitter,
    pub(crate) allow_ip_name_lookup: bool,
    tcp: SocketPolicy,
    udp: SocketPolicy,
    pub(crate) network_identity: Arc<()>,
}
impl WasiSocketsCtx {
    pub fn new(submitter: Submitter) -> Self {
        Self {
            submitter,
            allow_ip_name_lookup: false,
            network_identity: Arc::new(()),
            udp: SocketPolicy {
                allowed: false,
                check: Arc::new(|_, _| Box::pin(async { false })),
            },
            tcp: SocketPolicy {
                allowed: false,
                check: Arc::new(|_, _| Box::pin(async { false })),
            },
        }
    }

    /// Permit host name resolution. This capability defaults to denied;
    /// granting it does not grant TCP/UDP connection or bind permissions.
    pub fn allow_ip_name_lookup(&mut self, allow: bool) -> &mut Self {
        self.allow_ip_name_lookup = allow;
        self
    }

    /// Grant TCP operations. Individual addresses still require an explicit check.
    pub fn allow_tcp(&mut self, allow: bool) -> &mut Self {
        self.tcp.allowed = allow;
        self
    }

    /// Grant UDP operations. Binding and each destination still require a check.
    pub fn allow_udp(&mut self, allow: bool) -> &mut Self {
        self.udp.allowed = allow;
        self
    }

    /// Runs on the calling agent, including after native accept; the callback
    /// and its future are never submitted to the native I/O thread.
    pub fn socket_addr_check<F>(&mut self, check: F) -> &mut Self
    where
        F: Fn(SocketAddr, SocketAddrUse) -> Pin<Box<dyn Future<Output = bool> + Send>>
            + Send
            + Sync
            + 'static,
    {
        let check: AddressCheck = Arc::new(check);
        self.tcp.check = check.clone();
        self.udp.check = check;
        self
    }
}

pub struct WasiSocketsCtxView<'a> {
    pub ctx: &'a mut WasiSocketsCtx,
    pub table: &'a mut ResourceTable,
}

pub(crate) fn from_ipv4_addr(addr: std::net::Ipv4Addr) -> (u8, u8, u8, u8) {
    let [a, b, c, d] = addr.octets();
    (a, b, c, d)
}
pub(crate) fn to_ipv4_addr(addr: (u8, u8, u8, u8)) -> std::net::Ipv4Addr {
    std::net::Ipv4Addr::new(addr.0, addr.1, addr.2, addr.3)
}
pub(crate) fn from_ipv6_addr(addr: std::net::Ipv6Addr) -> (u16, u16, u16, u16, u16, u16, u16, u16) {
    let [a, b, c, d, e, f, g, h] = addr.segments();
    (a, b, c, d, e, f, g, h)
}
pub(crate) fn to_ipv6_addr(addr: (u16, u16, u16, u16, u16, u16, u16, u16)) -> std::net::Ipv6Addr {
    std::net::Ipv6Addr::new(
        addr.0, addr.1, addr.2, addr.3, addr.4, addr.5, addr.6, addr.7,
    )
}
