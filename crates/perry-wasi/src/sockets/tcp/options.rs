//! Fresh sockets have not yet been adopted by Turnloop. Configure them without
//! duplicating descriptors or changing the keepalive switch as a side effect.
use super::*;
use std::io;

pub(in crate::sockets) fn get(
    socket: &Socket,
    family: Family,
    kind: SocketOptionKind,
) -> io::Result<SocketOption> {
    Ok(match kind {
        SocketOptionKind::KeepAliveEnabled => SocketOption::KeepAliveEnabled(socket.keepalive()?),
        SocketOptionKind::KeepAliveSettings => {
            #[cfg(unix)]
            let settings = turnloop::KeepAlive {
                idle: Some(socket.tcp_keepalive_time()?),
                interval: Some(socket.tcp_keepalive_interval()?),
                count: Some(socket.tcp_keepalive_retries()?),
            };
            #[cfg(windows)]
            let settings = windows_keepalive(socket)?;
            SocketOption::KeepAliveSettings(settings)
        }
        SocketOptionKind::Ttl => SocketOption::Ttl(if family == Family::Ipv4 {
            socket.ttl_v4()?
        } else {
            socket.unicast_hops_v6()?
        }),
        SocketOptionKind::RecvBufferSize => SocketOption::RecvBufferSize(
            socket
                .recv_buffer_size()?
                .try_into()
                .map_err(|_| io::ErrorKind::InvalidData)?,
        ),
        SocketOptionKind::SendBufferSize => SocketOption::SendBufferSize(
            socket
                .send_buffer_size()?
                .try_into()
                .map_err(|_| io::ErrorKind::InvalidData)?,
        ),
        _ => return Err(io::ErrorKind::Unsupported.into()),
    })
}
pub(in crate::sockets) fn set(
    socket: &Socket,
    family: Family,
    option: SocketOption,
) -> io::Result<()> {
    match option {
        SocketOption::KeepAliveEnabled(value) => socket.set_keepalive(value),
        SocketOption::KeepAliveSettings(value) => {
            if let Some(value) = value.idle {
                schedule(socket, Schedule::Idle(value))?;
            }
            if let Some(value) = value.interval {
                schedule(socket, Schedule::Interval(value))?;
            }
            if let Some(value) = value.count {
                schedule(socket, Schedule::Count(value))?;
            }
            Ok(())
        }
        SocketOption::Ttl(value) => {
            if family == Family::Ipv4 {
                socket.set_ttl_v4(value)
            } else {
                socket.set_unicast_hops_v6(value)
            }
        }
        SocketOption::RecvBufferSize(value) => socket.set_recv_buffer_size(value as usize),
        SocketOption::SendBufferSize(value) => socket.set_send_buffer_size(value as usize),
        _ => Err(io::ErrorKind::Unsupported.into()),
    }
}
pub(super) fn same_setting(a: SocketOption, b: SocketOption) -> bool {
    match (a, b) {
        (SocketOption::KeepAliveSettings(a), SocketOption::KeepAliveSettings(b)) => {
            a.idle.is_some() == b.idle.is_some()
                && a.interval.is_some() == b.interval.is_some()
                && a.count.is_some() == b.count.is_some()
        }
        (a, b) => std::mem::discriminant(&a) == std::mem::discriminant(&b),
    }
}
enum Schedule {
    Idle(Duration),
    Interval(Duration),
    Count(u32),
}
#[cfg(unix)]
fn schedule(socket: &Socket, value: Schedule) -> io::Result<()> {
    use rustix::net::sockopt;
    match value {
        Schedule::Idle(value) => sockopt::set_tcp_keepidle(socket, value),
        Schedule::Interval(value) => sockopt::set_tcp_keepintvl(socket, value),
        Schedule::Count(value) => sockopt::set_tcp_keepcnt(socket, value),
    }
    .map_err(Into::into)
}
#[cfg(windows)]
fn windows_keepalive(socket: &Socket) -> io::Result<turnloop::KeepAlive> {
    use windows_sys::Win32::Networking::WinSock::{TCP_KEEPCNT, TCP_KEEPIDLE, TCP_KEEPINTVL};
    fn parameter(socket: &Socket, option: i32) -> io::Result<u32> {
        use std::os::windows::io::AsRawSocket;
        use windows_sys::Win32::Networking::WinSock::{
            getsockopt, WSAGetLastError, IPPROTO_TCP, SOCKET_ERROR,
        };
        let mut value = 0u32;
        let mut len = std::mem::size_of_val(&value) as i32;
        // SAFETY: the socket remains owned for this synchronous query; output
        // storage and its length describe exactly one initialized DWORD.
        let result = unsafe {
            getsockopt(
                socket.as_raw_socket() as usize,
                IPPROTO_TCP,
                option,
                (&raw mut value).cast(),
                &mut len,
            )
        };
        if result == SOCKET_ERROR {
            return Err(io::Error::from_raw_os_error(unsafe { WSAGetLastError() }));
        }
        if len != std::mem::size_of_val(&value) as i32 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        Ok(value)
    }
    Ok(turnloop::KeepAlive {
        idle: Some(Duration::from_secs(parameter(socket, TCP_KEEPIDLE)? as u64)),
        interval: Some(Duration::from_secs(parameter(socket, TCP_KEEPINTVL)? as u64)),
        count: Some(parameter(socket, TCP_KEEPCNT)?),
    })
}
#[cfg(windows)]
fn schedule(socket: &Socket, setting: Schedule) -> io::Result<()> {
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{
        setsockopt, WSAGetLastError, IPPROTO_TCP, SOCKET_ERROR, TCP_KEEPCNT, TCP_KEEPIDLE,
        TCP_KEEPINTVL,
    };
    let (option, value) = match setting {
        Schedule::Idle(value) => (TCP_KEEPIDLE, value.as_secs().max(1) as u32),
        Schedule::Interval(value) => (TCP_KEEPINTVL, value.as_secs().max(1) as u32),
        Schedule::Count(value) => (TCP_KEEPCNT, value),
    };
    // SAFETY: a live, exclusively owned TCP socket; value points to a valid
    // DWORD for the whole synchronous setsockopt call. No I/O is submitted.
    let result = unsafe {
        setsockopt(
            socket.as_raw_socket() as usize,
            IPPROTO_TCP,
            option,
            (&value as *const u32).cast(),
            std::mem::size_of_val(&value) as i32,
        )
    };
    if result == SOCKET_ERROR {
        // SAFETY: obtains the calling thread's last socket error immediately.
        Err(io::Error::from_raw_os_error(unsafe { WSAGetLastError() }))
    } else {
        Ok(())
    }
}
