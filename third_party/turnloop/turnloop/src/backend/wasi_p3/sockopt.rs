//! Typed socket options over `wasi:sockets` 0.3 (DESIGN §7.4, §7.7).
//!
//! WASI exposes a deliberately small option surface. Everything outside it —
//! Nagle, linger, IPv6-only, broadcast and multicast membership — has no
//! interface in `wasi:sockets` 0.3, so it is reported `Unsupported` rather than
//! accepted and dropped on the floor. Nothing is cached: each getter is an
//! import call, so a value the host clamped is the value reported back.
use super::{Socket, error};
use crate::{AcceptDefaults, Error, ErrorKind, KeepAlive, Result, SocketOption, SocketOptionKind};
use std::time::Duration;
use wasip3::sockets::types::TcpSocket;

fn unsupported<T>() -> Result<T> {
    Err(Error::new(ErrorKind::Unsupported))
}
fn invalid<T>() -> Result<T> {
    Err(Error::new(ErrorKind::InvalidInput))
}
fn hops(value: u32) -> Result<u8> {
    match u8::try_from(value) {
        Ok(0) | Err(_) => invalid(),
        Ok(hops) => Ok(hops),
    }
}
fn tcp(socket: &Socket) -> Result<&TcpSocket> {
    match socket {
        Socket::Tcp(socket) => Ok(socket),
        _ => unsupported(),
    }
}
/// WASI takes the schedule as a duration, so no rounding is needed; a zero
/// duration is still rejected, matching the native backends.
fn nonzero(value: Duration) -> Result<u64> {
    match u64::try_from(value.as_nanos()) {
        Ok(0) | Err(_) => invalid(),
        Ok(nanos) => Ok(nanos),
    }
}
fn set_keep_alive(socket: &TcpSocket, value: Option<KeepAlive>) -> Result<()> {
    let Some(schedule) = value else {
        return socket.set_keep_alive_enabled(false).map_err(error);
    };
    set_keep_alive_settings(socket, schedule, true)
}
fn set_keep_alive_settings(socket: &TcpSocket, schedule: KeepAlive, enable: bool) -> Result<()> {
    let idle = schedule.idle.map(nonzero).transpose()?;
    let interval = schedule.interval.map(nonzero).transpose()?;
    if schedule.count == Some(0) {
        return invalid();
    }
    if enable {
        socket.set_keep_alive_enabled(true).map_err(error)?;
    }
    if let Some(idle) = idle {
        socket.set_keep_alive_idle_time(idle).map_err(error)?;
    }
    if let Some(interval) = interval {
        socket.set_keep_alive_interval(interval).map_err(error)?;
    }
    if let Some(count) = schedule.count {
        socket.set_keep_alive_count(count).map_err(error)?;
    }
    Ok(())
}
fn get_keep_alive(socket: &TcpSocket) -> Result<Option<KeepAlive>> {
    if !socket.get_keep_alive_enabled().map_err(error)? {
        return Ok(None);
    }
    get_keep_alive_settings(socket).map(Some)
}
fn get_keep_alive_settings(socket: &TcpSocket) -> Result<KeepAlive> {
    Ok(KeepAlive {
        idle: Some(Duration::from_nanos(
            socket.get_keep_alive_idle_time().map_err(error)?,
        )),
        interval: Some(Duration::from_nanos(
            socket.get_keep_alive_interval().map_err(error)?,
        )),
        count: Some(socket.get_keep_alive_count().map_err(error)?),
    })
}
fn bytes(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// Apply one option to a live WASI socket.
pub(super) fn set(socket: &Socket, option: SocketOption) -> Result<()> {
    match option {
        SocketOption::KeepAlive(schedule) => set_keep_alive(tcp(socket)?, schedule),
        SocketOption::KeepAliveEnabled(on) => {
            tcp(socket)?.set_keep_alive_enabled(on).map_err(error)
        }
        SocketOption::KeepAliveSettings(schedule) => {
            set_keep_alive_settings(tcp(socket)?, schedule, false)
        }
        SocketOption::ListenBacklog(value) => {
            if value == 0 || value > i32::MAX as u32 {
                return invalid();
            }
            tcp(socket)?
                .set_listen_backlog_size(u64::from(value))
                .map_err(error)
        }
        SocketOption::RecvBufferSize(size) => match socket {
            Socket::Tcp(socket) => socket.set_receive_buffer_size(u64::from(size)),
            Socket::Udp(socket) => socket.set_receive_buffer_size(u64::from(size)),
            Socket::Stdio => return unsupported(),
        }
        .map_err(error),
        SocketOption::SendBufferSize(size) => match socket {
            Socket::Tcp(socket) => socket.set_send_buffer_size(u64::from(size)),
            Socket::Udp(socket) => socket.set_send_buffer_size(u64::from(size)),
            Socket::Stdio => return unsupported(),
        }
        .map_err(error),
        SocketOption::Ttl(value) => {
            let value = hops(value)?;
            match socket {
                Socket::Tcp(socket) => socket.set_hop_limit(value),
                Socket::Udp(socket) => socket.set_unicast_hop_limit(value),
                Socket::Stdio => return unsupported(),
            }
            .map_err(error)
        }
        // No interface in wasi:sockets@0.2.9.
        SocketOption::NoDelay(_)
        | SocketOption::Linger(_)
        | SocketOption::Ipv6Only(_)
        | SocketOption::Broadcast(_)
        | SocketOption::MulticastTtl(_)
        | SocketOption::MulticastLoop(_)
        | SocketOption::MulticastJoin(_)
        | SocketOption::MulticastLeave(_) => unsupported(),
    }
}
/// Read one option back through `wasi:sockets`.
pub(super) fn get(socket: &Socket, kind: SocketOptionKind) -> Result<SocketOption> {
    Ok(match kind {
        SocketOptionKind::KeepAlive => SocketOption::KeepAlive(get_keep_alive(tcp(socket)?)?),
        SocketOptionKind::KeepAliveEnabled => {
            SocketOption::KeepAliveEnabled(tcp(socket)?.get_keep_alive_enabled().map_err(error)?)
        }
        SocketOptionKind::KeepAliveSettings => {
            SocketOption::KeepAliveSettings(get_keep_alive_settings(tcp(socket)?)?)
        }
        SocketOptionKind::RecvBufferSize => SocketOption::RecvBufferSize(bytes(
            match socket {
                Socket::Tcp(socket) => socket.get_receive_buffer_size(),
                Socket::Udp(socket) => socket.get_receive_buffer_size(),
                Socket::Stdio => return unsupported(),
            }
            .map_err(error)?,
        )),
        SocketOptionKind::SendBufferSize => SocketOption::SendBufferSize(bytes(
            match socket {
                Socket::Tcp(socket) => socket.get_send_buffer_size(),
                Socket::Udp(socket) => socket.get_send_buffer_size(),
                Socket::Stdio => return unsupported(),
            }
            .map_err(error)?,
        )),
        SocketOptionKind::Ttl => SocketOption::Ttl(u32::from(
            match socket {
                Socket::Tcp(socket) => socket.get_hop_limit(),
                Socket::Udp(socket) => socket.get_unicast_hop_limit(),
                Socket::Stdio => return unsupported(),
            }
            .map_err(error)?,
        )),
        SocketOptionKind::NoDelay
        | SocketOptionKind::Linger
        | SocketOptionKind::Ipv6Only
        | SocketOptionKind::Broadcast
        | SocketOptionKind::MulticastTtl
        | SocketOptionKind::MulticastLoop => return unsupported(),
    })
}
/// Apply a listener's accepted-socket defaults to one accepted socket, before
/// the `Accepted` outcome exists.
pub(super) fn apply_accept_defaults(socket: &TcpSocket, defaults: AcceptDefaults) -> Result<()> {
    // `open` already refused this, but an accept must never quietly hand up a
    // connection whose requested default was not applied.
    if defaults.nodelay {
        return unsupported();
    }
    if let Some(schedule) = defaults.keep_alive {
        set_keep_alive(socket, Some(schedule))?;
    }
    Ok(())
}
/// Reject at listener creation what this backend could not apply per connection.
/// `wasi:sockets` has no Nagle control, so a `nodelay` default is refused here
/// instead of being ignored once per accepted connection.
pub(super) fn validate_accept_defaults(defaults: AcceptDefaults) -> Result<()> {
    if defaults.nodelay {
        return unsupported();
    }
    Ok(())
}
