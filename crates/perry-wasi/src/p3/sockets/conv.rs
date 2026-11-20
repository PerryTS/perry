// Adapted from Wasmtime 48.0.5; Apache-2.0 WITH LLVM-exception. See PROVENANCE.md.
use super::types;
use crate::sockets::{from_ipv4_addr, from_ipv6_addr, tcp::Family, to_ipv4_addr, to_ipv6_addr};
use std::net::{IpAddr, SocketAddr, SocketAddrV4, SocketAddrV6, ToSocketAddrs};
impl From<IpAddr> for types::IpAddress {
    fn from(addr: IpAddr) -> Self {
        match addr {
            IpAddr::V4(v4) => Self::Ipv4(from_ipv4_addr(v4)),
            IpAddr::V6(v6) => Self::Ipv6(from_ipv6_addr(v6)),
        }
    }
}

impl From<types::IpAddress> for IpAddr {
    fn from(addr: types::IpAddress) -> Self {
        match addr {
            types::IpAddress::Ipv4(v4) => Self::V4(to_ipv4_addr(v4)),
            types::IpAddress::Ipv6(v6) => Self::V6(to_ipv6_addr(v6)),
        }
    }
}

impl From<types::IpSocketAddress> for SocketAddr {
    fn from(addr: types::IpSocketAddress) -> Self {
        match addr {
            types::IpSocketAddress::Ipv4(ipv4) => Self::V4(ipv4.into()),
            types::IpSocketAddress::Ipv6(ipv6) => Self::V6(ipv6.into()),
        }
    }
}

impl From<SocketAddr> for types::IpSocketAddress {
    fn from(addr: SocketAddr) -> Self {
        match addr {
            SocketAddr::V4(v4) => Self::Ipv4(v4.into()),
            SocketAddr::V6(v6) => Self::Ipv6(v6.into()),
        }
    }
}

impl From<types::Ipv4SocketAddress> for SocketAddrV4 {
    fn from(addr: types::Ipv4SocketAddress) -> Self {
        Self::new(to_ipv4_addr(addr.address), addr.port)
    }
}

impl From<SocketAddrV4> for types::Ipv4SocketAddress {
    fn from(addr: SocketAddrV4) -> Self {
        Self {
            address: from_ipv4_addr(*addr.ip()),
            port: addr.port(),
        }
    }
}

impl From<types::Ipv6SocketAddress> for SocketAddrV6 {
    fn from(addr: types::Ipv6SocketAddress) -> Self {
        Self::new(
            to_ipv6_addr(addr.address),
            addr.port,
            addr.flow_info,
            addr.scope_id,
        )
    }
}

impl From<SocketAddrV6> for types::Ipv6SocketAddress {
    fn from(addr: SocketAddrV6) -> Self {
        Self {
            address: from_ipv6_addr(*addr.ip()),
            port: addr.port(),
            flow_info: addr.flowinfo(),
            scope_id: addr.scope_id(),
        }
    }
}

impl ToSocketAddrs for types::IpSocketAddress {
    type Iter = <SocketAddr as ToSocketAddrs>::Iter;

    fn to_socket_addrs(&self) -> std::io::Result<Self::Iter> {
        SocketAddr::from(*self).to_socket_addrs()
    }
}

impl ToSocketAddrs for types::Ipv4SocketAddress {
    type Iter = <SocketAddrV4 as ToSocketAddrs>::Iter;

    fn to_socket_addrs(&self) -> std::io::Result<Self::Iter> {
        SocketAddrV4::from(*self).to_socket_addrs()
    }
}

impl ToSocketAddrs for types::Ipv6SocketAddress {
    type Iter = <SocketAddrV6 as ToSocketAddrs>::Iter;

    fn to_socket_addrs(&self) -> std::io::Result<Self::Iter> {
        SocketAddrV6::from(*self).to_socket_addrs()
    }
}

impl From<Family> for types::IpAddressFamily {
    fn from(family: Family) -> Self {
        match family {
            Family::Ipv4 => Self::Ipv4,
            Family::Ipv6 => Self::Ipv6,
        }
    }
}

impl From<types::IpAddressFamily> for Family {
    fn from(family: types::IpAddressFamily) -> Self {
        match family {
            types::IpAddressFamily::Ipv4 => Self::Ipv4,
            types::IpAddressFamily::Ipv6 => Self::Ipv6,
        }
    }
}
