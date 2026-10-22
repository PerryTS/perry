//! Networking capabilities and owned native operations for WASI providers.
use perry_component_io::Submitter;
use wasmtime::component::{HasData, ResourceTable};

pub struct WasiSockets;
impl HasData for WasiSockets {
    type Data<'a> = WasiSocketsCtxView<'a>;
}

pub struct WasiSocketsCtx {
    pub(crate) submitter: Submitter,
    pub(crate) allow_ip_name_lookup: bool,
}
impl WasiSocketsCtx {
    pub fn new(submitter: Submitter) -> Self {
        Self {
            submitter,
            allow_ip_name_lookup: false,
        }
    }

    /// Permit host name resolution. This capability defaults to denied;
    /// granting it does not grant TCP/UDP connection or bind permissions.
    pub fn allow_ip_name_lookup(&mut self, allow: bool) -> &mut Self {
        self.allow_ip_name_lookup = allow;
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
