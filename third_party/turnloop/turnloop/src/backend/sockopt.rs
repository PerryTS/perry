//! Typed socket options on live Unix descriptors (DESIGN §7.1, §7.2, §7.7).
//!
//! Every entry point is synchronous, allocation-free and talks to the kernel on
//! the calling turn. Nothing is cached: a getter always asks the OS, so a value
//! the kernel rounded, clamped or doubled is reported as the kernel holds it.
//!
//! An option the platform lacks is reported, never ignored. Where the wrong
//! level would be used for a socket (`TCP_NODELAY` on UDP, an IP option on a
//! Unix-domain socket) the kernel's own `ENOPROTOOPT`/`EOPNOTSUPP` is returned
//! unchanged rather than being translated into a guess.
use super::poller::last_error;
use crate::{
    AcceptDefaults, Error, ErrorKind, KeepAlive, MulticastGroup, Result, SocketOption,
    SocketOptionKind,
};
use std::{mem::size_of, net::IpAddr, os::fd::RawFd, time::Duration};

/// Idle time before the first keep-alive probe. Apple spells it `TCP_KEEPALIVE`.
#[cfg(target_vendor = "apple")]
const KEEPIDLE: i32 = libc::TCP_KEEPALIVE;
#[cfg(not(target_vendor = "apple"))]
const KEEPIDLE: i32 = libc::TCP_KEEPIDLE;

#[cfg(any(target_os = "linux", target_os = "android"))]
const JOIN_V6: i32 = libc::IPV6_ADD_MEMBERSHIP;
#[cfg(any(target_os = "linux", target_os = "android"))]
const LEAVE_V6: i32 = libc::IPV6_DROP_MEMBERSHIP;
#[cfg(not(any(target_os = "linux", target_os = "android")))]
const JOIN_V6: i32 = libc::IPV6_JOIN_GROUP;
#[cfg(not(any(target_os = "linux", target_os = "android")))]
const LEAVE_V6: i32 = libc::IPV6_LEAVE_GROUP;

fn invalid() -> Error {
    Error::new(ErrorKind::InvalidInput)
}
fn unsupported() -> Error {
    Error::new(ErrorKind::Unsupported)
}
fn check(code: i32) -> Result<()> {
    if code < 0 {
        Err(last_error())
    } else {
        Ok(())
    }
}
fn set_int(fd: RawFd, level: i32, name: i32, value: i32) -> Result<()> {
    // SAFETY: value is an initialized integer of exactly the length passed.
    check(unsafe {
        libc::setsockopt(
            fd,
            level,
            name,
            std::ptr::from_ref(&value).cast(),
            size_of::<i32>() as libc::socklen_t,
        )
    })
}
fn get_int(fd: RawFd, level: i32, name: i32) -> Result<i32> {
    let mut value = 0i32;
    let mut len = size_of::<i32>() as libc::socklen_t;
    // SAFETY: writable initialized integer output with its exact capacity.
    check(unsafe {
        libc::getsockopt(
            fd,
            level,
            name,
            std::ptr::from_mut(&mut value).cast(),
            &mut len,
        )
    })?;
    Ok(value)
}
/// IPv4 multicast TTL/loop take a `u_char` on BSD and an `int` on Linux; both
/// widths are written and read explicitly instead of relying on byte order.
#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn set_v4_multicast(fd: RawFd, name: i32, value: u32) -> Result<()> {
    let value = u8::try_from(value).map_err(|_| invalid())?;
    // SAFETY: value is an initialized byte of exactly the length passed.
    check(unsafe {
        libc::setsockopt(
            fd,
            libc::IPPROTO_IP,
            name,
            std::ptr::from_ref(&value).cast(),
            size_of::<u8>() as libc::socklen_t,
        )
    })
}
#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn get_v4_multicast(fd: RawFd, name: i32) -> Result<u32> {
    let mut value = 0u8;
    let mut len = size_of::<u8>() as libc::socklen_t;
    // SAFETY: writable initialized byte output with its exact capacity.
    check(unsafe {
        libc::getsockopt(
            fd,
            libc::IPPROTO_IP,
            name,
            std::ptr::from_mut(&mut value).cast(),
            &mut len,
        )
    })?;
    Ok(u32::from(value))
}
#[cfg(any(target_os = "linux", target_os = "android"))]
fn set_v4_multicast(fd: RawFd, name: i32, value: u32) -> Result<()> {
    set_int(fd, libc::IPPROTO_IP, name, bounded(value)?)
}
#[cfg(any(target_os = "linux", target_os = "android"))]
fn get_v4_multicast(fd: RawFd, name: i32) -> Result<u32> {
    Ok(get_int(fd, libc::IPPROTO_IP, name)?.max(0) as u32)
}
fn bounded(value: u32) -> Result<i32> {
    i32::try_from(value).map_err(|_| invalid())
}
/// One-second granularity, rounded up: a caller asking for 1.2 s must not get 1 s.
fn whole_seconds(value: Duration) -> Result<i32> {
    let seconds = value
        .as_secs()
        .checked_add(u64::from(value.subsec_nanos() != 0))
        .ok_or_else(invalid)?;
    i32::try_from(seconds).map_err(|_| invalid())
}
/// True for an IPv6 socket, false for IPv4; anything else has no IP options.
fn ipv6(fd: RawFd) -> Result<bool> {
    // SAFETY: sockaddr_storage is plain C integer/padding storage; zero is valid.
    let mut storage: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut len = size_of::<libc::sockaddr_storage>() as libc::socklen_t;
    // SAFETY: live descriptor with writable address storage and its capacity.
    check(unsafe { libc::getsockname(fd, std::ptr::from_mut(&mut storage).cast(), &mut len) })?;
    match i32::from(storage.ss_family) {
        libc::AF_INET => Ok(false),
        libc::AF_INET6 => Ok(true),
        _ => Err(unsupported()),
    }
}
fn ip_level(fd: RawFd, v4: i32, v6: i32) -> Result<(i32, i32)> {
    if ipv6(fd)? {
        Ok((libc::IPPROTO_IPV6, v6))
    } else {
        Ok((libc::IPPROTO_IP, v4))
    }
}
fn set_linger(fd: RawFd, value: Option<Duration>) -> Result<()> {
    let value = libc::linger {
        l_onoff: i32::from(value.is_some()),
        l_linger: value.map_or(Ok(0), whole_seconds)?,
    };
    // SAFETY: a fully initialized `linger` of exactly the length passed.
    check(unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_LINGER,
            std::ptr::from_ref(&value).cast(),
            size_of::<libc::linger>() as libc::socklen_t,
        )
    })
}
fn get_linger(fd: RawFd) -> Result<Option<Duration>> {
    // SAFETY: `linger` is two plain C integers; zero is a valid initial value.
    let mut value: libc::linger = unsafe { std::mem::zeroed() };
    let mut len = size_of::<libc::linger>() as libc::socklen_t;
    // SAFETY: writable `linger` output with its exact capacity.
    check(unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_LINGER,
            std::ptr::from_mut(&mut value).cast(),
            &mut len,
        )
    })?;
    Ok((value.l_onoff != 0).then(|| Duration::from_secs(value.l_linger.max(0) as u64)))
}
/// Keep-alive is `SO_KEEPALIVE` plus up to three TCP-level schedule values.
/// Disabling clears the master switch and leaves the schedule untouched, exactly
/// as the OS does; the schedule is only written when probing is enabled.
fn set_keep_alive(fd: RawFd, value: Option<KeepAlive>) -> Result<()> {
    let Some(schedule) = value else {
        return set_int(fd, libc::SOL_SOCKET, libc::SO_KEEPALIVE, 0);
    };
    validate_keepalive(schedule)?;
    set_int(fd, libc::SOL_SOCKET, libc::SO_KEEPALIVE, 1)?;
    set_keepalive_parameters(fd, schedule)
}
fn validate_keepalive(schedule: KeepAlive) -> Result<(Option<i32>, Option<i32>, Option<i32>)> {
    let idle = schedule.idle.map(positive_seconds).transpose()?;
    let interval = schedule.interval.map(positive_seconds).transpose()?;
    let count = schedule.count.map(bounded).transpose()?;
    if count == Some(0) {
        return Err(invalid());
    }
    Ok((idle, interval, count))
}
pub(super) fn set_keepalive_parameters(fd: RawFd, schedule: KeepAlive) -> Result<()> {
    let (idle, interval, count) = validate_keepalive(schedule)?;
    if let Some(value) = idle {
        set_int(fd, libc::IPPROTO_TCP, KEEPIDLE, value)?;
    }
    if let Some(value) = interval {
        set_int(fd, libc::IPPROTO_TCP, libc::TCP_KEEPINTVL, value)?;
    }
    if let Some(value) = count {
        set_int(fd, libc::IPPROTO_TCP, libc::TCP_KEEPCNT, value)?;
    }
    Ok(())
}
fn get_keep_alive(fd: RawFd) -> Result<Option<KeepAlive>> {
    if get_int(fd, libc::SOL_SOCKET, libc::SO_KEEPALIVE)? == 0 {
        return Ok(None);
    }
    keepalive_parameters(fd).map(Some)
}
pub(super) fn keepalive_parameters(fd: RawFd) -> Result<KeepAlive> {
    Ok(KeepAlive {
        idle: Some(Duration::from_secs(
            get_int(fd, libc::IPPROTO_TCP, KEEPIDLE)?.max(0) as u64,
        )),
        interval: Some(Duration::from_secs(
            get_int(fd, libc::IPPROTO_TCP, libc::TCP_KEEPINTVL)?.max(0) as u64,
        )),
        count: Some(get_int(fd, libc::IPPROTO_TCP, libc::TCP_KEEPCNT)?.max(0) as u32),
    })
}
pub(super) fn set_listen_backlog(fd: RawFd, backlog: u32) -> Result<()> {
    let backlog = bounded(backlog)?;
    if backlog == 0 {
        return Err(invalid());
    }
    // SAFETY: this live socket was verified to be a listener; no pointer is passed.
    check(unsafe { libc::listen(fd, backlog) })
}
pub(super) fn shutdown_read(fd: RawFd) -> Result<()> {
    // SAFETY: the driver owns this live socket and has drained pending receive operations.
    check(unsafe { libc::shutdown(fd, libc::SHUT_RD) })
}
fn positive_seconds(value: Duration) -> Result<i32> {
    match whole_seconds(value)? {
        0 => Err(invalid()),
        seconds => Ok(seconds),
    }
}

fn membership(fd: RawFd, group: MulticastGroup, join: bool) -> Result<()> {
    match (group.group, ipv6(fd)?) {
        (IpAddr::V6(address), true) => {
            // SAFETY: `ipv6_mreq` is a plain address plus an index; zero is valid.
            let mut request: libc::ipv6_mreq = unsafe { std::mem::zeroed() };
            request.ipv6mr_multiaddr.s6_addr = address.octets();
            request.ipv6mr_interface = group.interface as _;
            let name = if join { JOIN_V6 } else { LEAVE_V6 };
            // SAFETY: a fully initialized request of exactly the length passed.
            check(unsafe {
                libc::setsockopt(
                    fd,
                    libc::IPPROTO_IPV6,
                    name,
                    std::ptr::from_ref(&request).cast(),
                    size_of::<libc::ipv6_mreq>() as libc::socklen_t,
                )
            })
        }
        (IpAddr::V4(address), false) => {
            let name = if join {
                libc::IP_ADD_MEMBERSHIP
            } else {
                libc::IP_DROP_MEMBERSHIP
            };
            if group.interface == 0 {
                // SAFETY: `ip_mreq` is two plain addresses; zero is valid.
                let mut request: libc::ip_mreq = unsafe { std::mem::zeroed() };
                request.imr_multiaddr.s_addr = u32::from_ne_bytes(address.octets());
                request.imr_interface.s_addr = libc::INADDR_ANY.to_be();
                // SAFETY: a fully initialized request of exactly the length passed.
                return check(unsafe {
                    libc::setsockopt(
                        fd,
                        libc::IPPROTO_IP,
                        name,
                        std::ptr::from_ref(&request).cast(),
                        size_of::<libc::ip_mreq>() as libc::socklen_t,
                    )
                });
            }
            interface_membership(fd, address, group.interface, name)
        }
        _ => Err(invalid()),
    }
}
/// Only Linux accepts an IPv4 membership keyed by interface index (`ip_mreqn`).
/// Elsewhere the request is reported, not silently applied to the default route.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn interface_membership(
    fd: RawFd,
    address: std::net::Ipv4Addr,
    interface: u32,
    name: i32,
) -> Result<()> {
    // SAFETY: `ip_mreqn` is two plain addresses and an index; zero is valid.
    let mut request: libc::ip_mreqn = unsafe { std::mem::zeroed() };
    request.imr_multiaddr.s_addr = u32::from_ne_bytes(address.octets());
    request.imr_ifindex = bounded(interface)?;
    // SAFETY: a fully initialized request of exactly the length passed.
    check(unsafe {
        libc::setsockopt(
            fd,
            libc::IPPROTO_IP,
            name,
            std::ptr::from_ref(&request).cast(),
            size_of::<libc::ip_mreqn>() as libc::socklen_t,
        )
    })
}
#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn interface_membership(
    _fd: RawFd,
    _address: std::net::Ipv4Addr,
    _interface: u32,
    _name: i32,
) -> Result<()> {
    Err(unsupported())
}

/// Apply one option to a live descriptor.
pub(super) fn set(fd: RawFd, option: SocketOption) -> Result<()> {
    match option {
        SocketOption::NoDelay(on) => {
            set_int(fd, libc::IPPROTO_TCP, libc::TCP_NODELAY, i32::from(on))
        }
        SocketOption::KeepAlive(schedule) => set_keep_alive(fd, schedule),
        SocketOption::KeepAliveEnabled(on) => {
            set_int(fd, libc::SOL_SOCKET, libc::SO_KEEPALIVE, i32::from(on))
        }
        SocketOption::KeepAliveSettings(schedule) => set_keepalive_parameters(fd, schedule),
        SocketOption::ListenBacklog(value) => {
            let value = bounded(value)?;
            if value == 0 {
                return Err(invalid());
            }
            // SAFETY: the backend verified this is a live TCP listener; no ownership transfer.
            check(unsafe { libc::listen(fd, value) })
        }
        SocketOption::Linger(value) => set_linger(fd, value),
        SocketOption::RecvBufferSize(bytes) => {
            set_int(fd, libc::SOL_SOCKET, libc::SO_RCVBUF, bounded(bytes)?)
        }
        SocketOption::SendBufferSize(bytes) => {
            set_int(fd, libc::SOL_SOCKET, libc::SO_SNDBUF, bounded(bytes)?)
        }
        SocketOption::Ttl(hops) => {
            let (level, name) = ip_level(fd, libc::IP_TTL, libc::IPV6_UNICAST_HOPS)?;
            set_int(fd, level, name, bounded(hops)?)
        }
        SocketOption::Ipv6Only(on) => {
            set_int(fd, libc::IPPROTO_IPV6, libc::IPV6_V6ONLY, i32::from(on))
        }
        SocketOption::Broadcast(on) => {
            set_int(fd, libc::SOL_SOCKET, libc::SO_BROADCAST, i32::from(on))
        }
        SocketOption::MulticastTtl(hops) => {
            if ipv6(fd)? {
                set_int(
                    fd,
                    libc::IPPROTO_IPV6,
                    libc::IPV6_MULTICAST_HOPS,
                    bounded(hops)?,
                )
            } else {
                set_v4_multicast(fd, libc::IP_MULTICAST_TTL, hops)
            }
        }
        SocketOption::MulticastLoop(on) => {
            if ipv6(fd)? {
                set_int(
                    fd,
                    libc::IPPROTO_IPV6,
                    libc::IPV6_MULTICAST_LOOP,
                    i32::from(on),
                )
            } else {
                set_v4_multicast(fd, libc::IP_MULTICAST_LOOP, u32::from(on))
            }
        }
        SocketOption::MulticastJoin(group) => membership(fd, group, true),
        SocketOption::MulticastLeave(group) => membership(fd, group, false),
    }
}
/// Read one option back from the kernel.
pub(super) fn get(fd: RawFd, kind: SocketOptionKind) -> Result<SocketOption> {
    Ok(match kind {
        SocketOptionKind::NoDelay => {
            SocketOption::NoDelay(get_int(fd, libc::IPPROTO_TCP, libc::TCP_NODELAY)? != 0)
        }
        SocketOptionKind::KeepAlive => SocketOption::KeepAlive(get_keep_alive(fd)?),
        SocketOptionKind::KeepAliveEnabled => {
            SocketOption::KeepAliveEnabled(get_int(fd, libc::SOL_SOCKET, libc::SO_KEEPALIVE)? != 0)
        }
        SocketOptionKind::KeepAliveSettings => {
            SocketOption::KeepAliveSettings(keepalive_parameters(fd)?)
        }
        SocketOptionKind::Linger => SocketOption::Linger(get_linger(fd)?),
        SocketOptionKind::RecvBufferSize => SocketOption::RecvBufferSize(
            get_int(fd, libc::SOL_SOCKET, libc::SO_RCVBUF)?.max(0) as u32,
        ),
        SocketOptionKind::SendBufferSize => SocketOption::SendBufferSize(
            get_int(fd, libc::SOL_SOCKET, libc::SO_SNDBUF)?.max(0) as u32,
        ),
        SocketOptionKind::Ttl => {
            let (level, name) = ip_level(fd, libc::IP_TTL, libc::IPV6_UNICAST_HOPS)?;
            SocketOption::Ttl(get_int(fd, level, name)?.max(0) as u32)
        }
        SocketOptionKind::Ipv6Only => {
            SocketOption::Ipv6Only(get_int(fd, libc::IPPROTO_IPV6, libc::IPV6_V6ONLY)? != 0)
        }
        SocketOptionKind::Broadcast => {
            SocketOption::Broadcast(get_int(fd, libc::SOL_SOCKET, libc::SO_BROADCAST)? != 0)
        }
        SocketOptionKind::MulticastTtl => SocketOption::MulticastTtl(if ipv6(fd)? {
            get_int(fd, libc::IPPROTO_IPV6, libc::IPV6_MULTICAST_HOPS)?.max(0) as u32
        } else {
            get_v4_multicast(fd, libc::IP_MULTICAST_TTL)?
        }),
        SocketOptionKind::MulticastLoop => SocketOption::MulticastLoop(if ipv6(fd)? {
            get_int(fd, libc::IPPROTO_IPV6, libc::IPV6_MULTICAST_LOOP)? != 0
        } else {
            get_v4_multicast(fd, libc::IP_MULTICAST_LOOP)? != 0
        }),
    })
}
/// Apply a listener's accepted-socket defaults to one freshly accepted socket.
/// Called before the `Accepted` outcome exists, so a failure fails that accept
/// and closes the connection instead of handing up a half-configured socket.
pub(super) fn apply_accept_defaults(fd: RawFd, defaults: AcceptDefaults) -> Result<()> {
    if defaults.nodelay {
        set(fd, SocketOption::NoDelay(true))?;
    }
    if let Some(schedule) = defaults.keep_alive {
        set(fd, SocketOption::KeepAlive(Some(schedule)))?;
    }
    Ok(())
}
/// Reject at listener creation what this backend could not apply per connection.
/// Both native defaults are TCP options, so only a local (AF_UNIX) listener has
/// to refuse them.
pub(super) fn validate_accept_defaults(defaults: AcceptDefaults, tcp: bool) -> Result<()> {
    if tcp || defaults.is_empty() {
        Ok(())
    } else {
        Err(unsupported())
    }
}
