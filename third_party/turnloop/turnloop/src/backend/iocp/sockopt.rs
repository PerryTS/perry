//! Typed socket options on live Winsock sockets (DESIGN §7.3, §7.7).
//!
//! Synchronous, allocation-free and uncached, exactly like the Unix module: a
//! getter always asks Winsock, so a buffer size the stack rounded is visible.
//! Winsock reports a wrong-level option itself (`WSAENOPROTOOPT`), and that error
//! is returned unchanged rather than guessed at.
use super::{Kind, invalid, unsupported};
use crate::{
    AcceptDefaults, Error, KeepAlive, MulticastGroup, Result, SocketOption, SocketOptionKind,
};
use std::{mem::size_of, net::IpAddr, ptr, time::Duration};
use windows_sys::Win32::Networking::WinSock::*;

fn set_int(socket: usize, level: i32, name: i32, value: i32) -> Result<()> {
    // SAFETY: value is an initialized integer of exactly the length passed.
    super::socket::check(unsafe {
        setsockopt(
            socket,
            level,
            name,
            ptr::from_ref(&value).cast(),
            size_of::<i32>() as i32,
        )
    })
}
fn get_int(socket: usize, level: i32, name: i32) -> Result<i32> {
    let mut value = 0i32;
    let mut len = size_of::<i32>() as i32;
    // SAFETY: writable initialized integer output with its exact capacity.
    super::socket::check(unsafe {
        getsockopt(
            socket,
            level,
            name,
            ptr::from_mut(&mut value).cast(),
            &mut len,
        )
    })?;
    Ok(value)
}
fn bounded(value: u32) -> Result<i32> {
    i32::try_from(value).map_err(|_| invalid())
}
/// One-second granularity, rounded up: a caller asking for 1.2 s must not get 1 s.
fn whole_seconds(value: Duration) -> Result<u32> {
    let seconds = value
        .as_secs()
        .checked_add(u64::from(value.subsec_nanos() != 0))
        .ok_or_else(invalid)?;
    u32::try_from(seconds).map_err(|_| invalid())
}
fn positive_seconds(value: Duration) -> Result<u32> {
    match whole_seconds(value)? {
        0 => Err(invalid()),
        seconds => Ok(seconds),
    }
}
/// True for an IPv6 socket, false for IPv4; anything else has no IP options.
fn ipv6(socket: usize) -> Result<bool> {
    // SAFETY: plain C address storage; zero is a valid initial value.
    let mut storage: SOCKADDR_STORAGE = unsafe { std::mem::zeroed() };
    let mut len = size_of::<SOCKADDR_STORAGE>() as i32;
    // SAFETY: live socket with writable address storage and its capacity.
    super::socket::check(unsafe {
        getsockname(socket, ptr::from_mut(&mut storage).cast(), &mut len)
    })?;
    match storage.ss_family {
        AF_INET => Ok(false),
        AF_INET6 => Ok(true),
        _ => Err(unsupported()),
    }
}
fn ip_level(socket: usize, v4: i32, v6: i32) -> Result<(i32, i32)> {
    if ipv6(socket)? {
        Ok((IPPROTO_IPV6, v6))
    } else {
        Ok((IPPROTO_IP, v4))
    }
}
fn set_linger(socket: usize, value: Option<Duration>) -> Result<()> {
    let seconds = value.map_or(Ok(0), whole_seconds)?;
    let value = LINGER {
        l_onoff: u16::from(value.is_some()),
        l_linger: u16::try_from(seconds).map_err(|_| invalid())?,
    };
    // SAFETY: a fully initialized LINGER of exactly the length passed.
    super::socket::check(unsafe {
        setsockopt(
            socket,
            SOL_SOCKET,
            SO_LINGER,
            ptr::from_ref(&value).cast(),
            size_of::<LINGER>() as i32,
        )
    })
}
fn get_linger(socket: usize) -> Result<Option<Duration>> {
    let mut value = LINGER::default();
    let mut len = size_of::<LINGER>() as i32;
    // SAFETY: writable LINGER output with its exact capacity.
    super::socket::check(unsafe {
        getsockopt(
            socket,
            SOL_SOCKET,
            SO_LINGER,
            ptr::from_mut(&mut value).cast(),
            &mut len,
        )
    })?;
    Ok((value.l_onoff != 0).then(|| Duration::from_secs(u64::from(value.l_linger))))
}
/// `SO_KEEPALIVE` plus the three TCP schedule values Windows 10 1709 added.
/// Disabling clears the master switch and leaves the schedule alone, as the OS does.
fn set_keep_alive(socket: usize, value: Option<KeepAlive>) -> Result<()> {
    let Some(schedule) = value else {
        return set_int(socket, SOL_SOCKET, SO_KEEPALIVE, 0);
    };
    set_keep_alive_settings(socket, schedule, true)
}
fn set_keep_alive_settings(socket: usize, schedule: KeepAlive, enable: bool) -> Result<()> {
    let idle = schedule
        .idle
        .map(|v| bounded(positive_seconds(v)?))
        .transpose()?;
    let interval = schedule
        .interval
        .map(|v| bounded(positive_seconds(v)?))
        .transpose()?;
    let count = schedule.count.map(bounded).transpose()?;
    if count == Some(0) {
        return Err(invalid());
    }
    if enable {
        set_int(socket, SOL_SOCKET, SO_KEEPALIVE, 1)?;
    }
    if let Some(idle) = idle {
        set_int(socket, IPPROTO_TCP, TCP_KEEPIDLE, idle)?;
    }
    if let Some(interval) = interval {
        set_int(socket, IPPROTO_TCP, TCP_KEEPINTVL, interval)?;
    }
    if let Some(count) = count {
        set_int(socket, IPPROTO_TCP, TCP_KEEPCNT, count)?;
    }
    Ok(())
}
fn get_keep_alive(socket: usize) -> Result<Option<KeepAlive>> {
    if get_int(socket, SOL_SOCKET, SO_KEEPALIVE)? == 0 {
        return Ok(None);
    }
    get_keep_alive_settings(socket).map(Some)
}
fn get_keep_alive_settings(socket: usize) -> Result<KeepAlive> {
    Ok(KeepAlive {
        idle: Some(Duration::from_secs(
            get_int(socket, IPPROTO_TCP, TCP_KEEPIDLE)?.max(0) as u64,
        )),
        interval: Some(Duration::from_secs(
            get_int(socket, IPPROTO_TCP, TCP_KEEPINTVL)?.max(0) as u64,
        )),
        count: Some(get_int(socket, IPPROTO_TCP, TCP_KEEPCNT)?.max(0) as u32),
    })
}
fn membership(socket: usize, group: MulticastGroup, join: bool) -> Result<()> {
    match (group.group, ipv6(socket)?) {
        (IpAddr::V6(address), true) => {
            let mut request = IPV6_MREQ::default();
            request.ipv6mr_multiaddr.u.Byte = address.octets();
            request.ipv6mr_interface = group.interface;
            let name = if join {
                IPV6_ADD_MEMBERSHIP
            } else {
                IPV6_DROP_MEMBERSHIP
            };
            // SAFETY: a fully initialized request of exactly the length passed.
            super::socket::check(unsafe {
                setsockopt(
                    socket,
                    IPPROTO_IPV6,
                    name,
                    ptr::from_ref(&request).cast(),
                    size_of::<IPV6_MREQ>() as i32,
                )
            })
        }
        // Winsock's IPv4 membership names the interface by address, not index, so
        // a nonzero index is reported instead of being applied to the default one.
        (IpAddr::V4(_), false) if group.interface != 0 => Err(unsupported()),
        (IpAddr::V4(address), false) => {
            let mut request = IP_MREQ::default();
            request.imr_multiaddr.S_un.S_addr = u32::from_ne_bytes(address.octets());
            request.imr_interface.S_un.S_addr = 0;
            let name = if join {
                IP_ADD_MEMBERSHIP
            } else {
                IP_DROP_MEMBERSHIP
            };
            // SAFETY: a fully initialized request of exactly the length passed.
            super::socket::check(unsafe {
                setsockopt(
                    socket,
                    IPPROTO_IP,
                    name,
                    ptr::from_ref(&request).cast(),
                    size_of::<IP_MREQ>() as i32,
                )
            })
        }
        _ => Err(invalid()),
    }
}

/// Apply one option to a live socket.
pub(super) fn set(socket: usize, option: SocketOption) -> Result<()> {
    match option {
        SocketOption::NoDelay(on) => set_int(socket, IPPROTO_TCP, TCP_NODELAY, i32::from(on)),
        SocketOption::KeepAlive(schedule) => set_keep_alive(socket, schedule),
        SocketOption::KeepAliveEnabled(on) => {
            set_int(socket, SOL_SOCKET, SO_KEEPALIVE, i32::from(on))
        }
        SocketOption::KeepAliveSettings(schedule) => {
            set_keep_alive_settings(socket, schedule, false)
        }
        SocketOption::ListenBacklog(value) => {
            let value = bounded(value)?;
            if value == 0 {
                return Err(invalid());
            }
            // SAFETY: the backend verified this is a live TCP listener; no ownership transfer.
            super::socket::check(unsafe { listen(socket, value) })
        }
        SocketOption::Linger(value) => set_linger(socket, value),
        SocketOption::RecvBufferSize(bytes) => {
            set_int(socket, SOL_SOCKET, SO_RCVBUF, bounded(bytes)?)
        }
        SocketOption::SendBufferSize(bytes) => {
            set_int(socket, SOL_SOCKET, SO_SNDBUF, bounded(bytes)?)
        }
        SocketOption::Ttl(hops) => {
            let (level, name) = ip_level(socket, IP_TTL, IPV6_UNICAST_HOPS)?;
            set_int(socket, level, name, bounded(hops)?)
        }
        SocketOption::Ipv6Only(on) => set_int(socket, IPPROTO_IPV6, IPV6_V6ONLY, i32::from(on)),
        SocketOption::Broadcast(on) => set_int(socket, SOL_SOCKET, SO_BROADCAST, i32::from(on)),
        SocketOption::MulticastTtl(hops) => {
            let (level, name) = ip_level(socket, IP_MULTICAST_TTL, IPV6_MULTICAST_HOPS)?;
            set_int(socket, level, name, bounded(hops)?)
        }
        SocketOption::MulticastLoop(on) => {
            let (level, name) = ip_level(socket, IP_MULTICAST_LOOP, IPV6_MULTICAST_LOOP)?;
            set_int(socket, level, name, i32::from(on))
        }
        SocketOption::MulticastJoin(group) => membership(socket, group, true),
        SocketOption::MulticastLeave(group) => membership(socket, group, false),
    }
}
/// Read one option back from Winsock.
pub(super) fn get(socket: usize, kind: SocketOptionKind) -> Result<SocketOption> {
    Ok(match kind {
        SocketOptionKind::NoDelay => {
            SocketOption::NoDelay(get_int(socket, IPPROTO_TCP, TCP_NODELAY)? != 0)
        }
        SocketOptionKind::KeepAlive => SocketOption::KeepAlive(get_keep_alive(socket)?),
        SocketOptionKind::KeepAliveEnabled => {
            SocketOption::KeepAliveEnabled(get_int(socket, SOL_SOCKET, SO_KEEPALIVE)? != 0)
        }
        SocketOptionKind::KeepAliveSettings => {
            SocketOption::KeepAliveSettings(get_keep_alive_settings(socket)?)
        }
        SocketOptionKind::Linger => SocketOption::Linger(get_linger(socket)?),
        SocketOptionKind::RecvBufferSize => {
            SocketOption::RecvBufferSize(get_int(socket, SOL_SOCKET, SO_RCVBUF)?.max(0) as u32)
        }
        SocketOptionKind::SendBufferSize => {
            SocketOption::SendBufferSize(get_int(socket, SOL_SOCKET, SO_SNDBUF)?.max(0) as u32)
        }
        SocketOptionKind::Ttl => {
            let (level, name) = ip_level(socket, IP_TTL, IPV6_UNICAST_HOPS)?;
            SocketOption::Ttl(get_int(socket, level, name)?.max(0) as u32)
        }
        SocketOptionKind::Ipv6Only => {
            SocketOption::Ipv6Only(get_int(socket, IPPROTO_IPV6, IPV6_V6ONLY)? != 0)
        }
        SocketOptionKind::Broadcast => {
            SocketOption::Broadcast(get_int(socket, SOL_SOCKET, SO_BROADCAST)? != 0)
        }
        SocketOptionKind::MulticastTtl => {
            let (level, name) = ip_level(socket, IP_MULTICAST_TTL, IPV6_MULTICAST_HOPS)?;
            SocketOption::MulticastTtl(get_int(socket, level, name)?.max(0) as u32)
        }
        SocketOptionKind::MulticastLoop => {
            let (level, name) = ip_level(socket, IP_MULTICAST_LOOP, IPV6_MULTICAST_LOOP)?;
            SocketOption::MulticastLoop(get_int(socket, level, name)? != 0)
        }
    })
}
/// Apply a listener's accepted-socket defaults to one accepted socket, after
/// `SO_UPDATE_ACCEPT_CONTEXT` (which is what makes the socket queryable) and
/// before the `Accepted` outcome exists.
pub(super) fn apply_accept_defaults(socket: usize, defaults: AcceptDefaults) -> Result<()> {
    if defaults.nodelay {
        set(socket, SocketOption::NoDelay(true))?;
    }
    if let Some(schedule) = defaults.keep_alive {
        set(socket, SocketOption::KeepAlive(Some(schedule)))?;
    }
    Ok(())
}
/// Reject at listener creation what this backend could not apply per connection.
/// Both native defaults are TCP options, so a named-pipe listener refuses them.
pub(super) fn validate_accept_defaults(defaults: AcceptDefaults, kind: Kind) -> Result<()> {
    if kind == Kind::Listener || defaults.is_empty() {
        Ok(())
    } else {
        Err(Error::new(crate::ErrorKind::Unsupported))
    }
}
