//! Own native operations while an optional WASI host borrows the process driver.
//! The embedding supplies exclusive driver access and completion extraction.
//! The standalone feature supplies one shared driver per embedding thread.
//! Guest waits select their own completions without dispatching callbacks.
use perry_abi::{component_host::Buffer, component_io as abi};
use std::{
    marker::PhantomData,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6},
    ptr,
    time::Duration,
};
use turnloop::{
    Completion, Error, ErrorKind, Handle, Loop, OpId, OpResult, ReadBuf, Timeout, Token, WriteBuf,
};

pub const TOKEN: Token = Token(0x40 << 56);
pub trait Embedding: 'static {
    fn with_driver<R>(f: impl FnOnce(&mut Loop) -> turnloop::Result<R>) -> turnloop::Result<R>;
    fn turn_and_take(
        operations: &[OpId],
        closing: &[Handle],
        timeout: Timeout,
    ) -> turnloop::Result<Vec<Completion>>;
}
pub struct Provider<E>(PhantomData<E>);
impl<E: Embedding> Provider<E> {
    pub const OPS: abi::Ops = abi::Ops {
        abi_version: abi::ABI_VERSION,
        size: std::mem::size_of::<abi::Ops>(),
        adopt: adopt::<E>,
        submit: submit::<E>,
        poll: poll::<E>,
        ready,
        take: take::<E>,
        operation_drop: operation_drop::<E>,
        resource_drop: resource_drop::<E>,
        completion_drop: completion_drop::<E>,
        raw_socket: raw_socket::<E>,
        release_socket: release_socket::<E>,
        option: option::<E>,
    };
}
struct NativeResource(Handle);
struct NativeOperation {
    id: OpId,
    completion: Option<Completion>,
    buffer: Vec<u8>,
    timer: Option<Handle>,
}
fn invalid() -> Error {
    Error::new(ErrorKind::InvalidInput)
}
fn error(e: Error) -> abi::Error {
    use ErrorKind as K;
    abi::Error {
        kind: match e.kind {
            K::Cancelled => abi::ERR_CANCELLED,
            K::Unsupported => abi::ERR_UNSUPPORTED,
            K::InvalidInput => abi::ERR_INVALID,
            K::NotFound => abi::ERR_NOT_FOUND,
            K::WouldBlock => abi::ERR_WOULD_BLOCK,
            K::TimedOut => abi::ERR_TIMEOUT,
            K::ConnectionRefused => abi::ERR_REFUSED,
            K::ConnectionReset => abi::ERR_RESET,
            K::BrokenPipe => abi::ERR_PIPE,
            K::ResourceLimit => abi::ERR_LIMIT,
            K::PermissionDenied => abi::ERR_ACCESS,
            _ => abi::ERR_OTHER,
        },
        os: e.os.unwrap_or(0),
    }
}
fn status(result: turnloop::Result<()>) -> abi::Error {
    result.err().map(error).unwrap_or_default()
}
fn address(a: abi::Address) -> turnloop::Result<SocketAddr> {
    Ok(match a.family {
        4 => SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(
                a.octets[0],
                a.octets[1],
                a.octets[2],
                a.octets[3],
            )),
            a.port,
        ),
        6 => SocketAddr::V6(SocketAddrV6::new(
            Ipv6Addr::from(a.octets),
            a.port,
            a.flow_info,
            a.scope_id,
        )),
        _ => return Err(invalid()),
    })
}
fn encode_address(a: SocketAddr) -> abi::Address {
    let mut out = abi::Address {
        port: a.port(),
        ..Default::default()
    };
    match a {
        SocketAddr::V4(a) => {
            out.family = 4;
            out.octets[..4].copy_from_slice(&a.ip().octets());
        }
        SocketAddr::V6(a) => {
            out.family = 6;
            out.octets = a.ip().octets();
            out.flow_info = a.flowinfo();
            out.scope_id = a.scope_id();
        }
    }
    out
}
fn native_resource(h: Handle) -> abi::Opaque {
    Box::into_raw(Box::new(NativeResource(h))).cast()
}
fn native_operation(id: OpId, buffer: Vec<u8>, timer: Option<Handle>) -> abi::Opaque {
    Box::into_raw(Box::new(NativeOperation {
        id,
        completion: None,
        buffer,
        timer,
    }))
    .cast()
}
unsafe fn resource(p: abi::Opaque) -> turnloop::Result<Handle> {
    if p.is_null() {
        return Err(invalid());
    }
    // SAFETY: the ABI caller holds the unconsumed resource from this table.
    Ok(unsafe { &*p.cast::<NativeResource>() }.0)
}
#[cfg(unix)]
unsafe fn detached(raw: usize, listener: bool) -> turnloop::Result<turnloop::Detached> {
    use std::os::fd::{FromRawFd, OwnedFd};
    // SAFETY: adopt transfers ownership of this native descriptor.
    let fd = unsafe { OwnedFd::from_raw_fd(raw as i32) };
    if listener {
        turnloop::Detached::from_listener_fd(fd)
    } else {
        turnloop::Detached::from_fd(fd)
    }
}
#[cfg(windows)]
unsafe fn detached(raw: usize, _: bool) -> turnloop::Result<turnloop::Detached> {
    use std::os::windows::io::{FromRawSocket, OwnedSocket};
    // SAFETY: adopt transfers ownership of this native socket.
    turnloop::Detached::from_socket(unsafe { OwnedSocket::from_raw_socket(raw as _) })
}
/// Query the actual native endpoint without mutating or taking its ownership.
unsafe fn socket_is_unbound(raw: usize) -> turnloop::Result<bool> {
    #[cfg(unix)]
    let stream = {
        use std::os::fd::FromRawFd;
        // SAFETY: adopt owns this live TCP descriptor; ManuallyDrop is a
        // temporary query view and never closes the owning Detached's socket.
        std::mem::ManuallyDrop::new(unsafe { std::net::TcpStream::from_raw_fd(raw as i32) })
    };
    #[cfg(windows)]
    let stream = {
        use std::os::windows::io::FromRawSocket;
        // SAFETY: same borrowed query view of the live, owned Winsock socket.
        std::mem::ManuallyDrop::new(unsafe { std::net::TcpStream::from_raw_socket(raw as _) })
    };
    match stream.local_addr() {
        Ok(local) => Ok(local.port() == 0),
        // WSAEINVAL: getsockname has no endpoint before the first bind.
        #[cfg(windows)]
        Err(error) if error.raw_os_error() == Some(10022) => Ok(true),
        Err(error) => Err(error.into()),
    }
}
unsafe extern "C" fn adopt<E: Embedding>(
    raw: usize,
    listener: bool,
    destination: *const abi::Address,
    res: *mut abi::Opaque,
    op: *mut abi::Opaque,
) -> abi::Error {
    // SAFETY: pointers obey the Ops contract; detached consumes the native socket.
    status(unsafe {
        (|| {
            *res = ptr::null_mut();
            *op = ptr::null_mut();
            let detached = detached(raw, listener)?;
            if destination.is_null() {
                let h = E::with_driver(|d| d.attach(detached, TOKEN))?;
                *res = native_resource(h);
            } else {
                let address = address(*destination)?;
                let unbound = socket_is_unbound(raw)?;
                let (h, id) = E::with_driver(|d| {
                    if unbound {
                        d.tcp_connect_unbound(detached, address, TOKEN)
                    } else {
                        d.tcp_connect_bound(detached, address, TOKEN)
                    }
                })?;
                *res = native_resource(h);
                *op = native_operation(id, Vec::new(), None);
            }
            Ok(())
        })()
    })
}
unsafe extern "C" fn submit<E: Embedding>(
    res: abi::Opaque,
    kind: u32,
    bytes: *const u8,
    len: usize,
    addr: *const abi::Address,
    delay: u64,
    out: *mut abi::Opaque,
) -> abi::Error {
    // SAFETY: pointers obey the Ops contract. Read buffers stay owned by the
    // NativeOperation until a terminal completion is extracted.
    status(unsafe {
        (|| {
            *out = ptr::null_mut();
            let mut buffer = Vec::new();
            let mut timer = None;
            let id = match kind {
                abi::TIMER => E::with_driver(|d| {
                    let at = d
                        .now()
                        .checked_add(Duration::from_nanos(delay))
                        .ok_or_else(invalid)?;
                    let h = d.timer(at, None, TOKEN)?;
                    timer = Some(h);
                    Ok(d.timer_op(h).expect("new timer has its operation"))
                })?,
                abi::RESOLVE => {
                    let data = if len == 0 {
                        &[]
                    } else {
                        std::slice::from_raw_parts(bytes, len)
                    };
                    let host = std::str::from_utf8(data).map_err(|_| invalid())?.to_owned();
                    E::with_driver(|d| d.resolve(turnloop::DnsRequest { host, port: 0 }, TOKEN))?
                }
                abi::READ | abi::RECEIVE => {
                    let handle = resource(res)?;
                    buffer.resize(len.min(64 * 1024 * 1024), 0);
                    let native =
                        turnloop::IoBufMut::from_raw_parts(buffer.as_mut_ptr(), buffer.len());
                    E::with_driver(|d| {
                        if kind == abi::READ {
                            d.read(handle, ReadBuf::Provided(native), TOKEN)
                        } else {
                            d.recv(handle, ReadBuf::Provided(native), TOKEN)
                        }
                    })?
                }
                abi::WRITE | abi::SEND => {
                    let handle = resource(res)?;
                    let data = if len == 0 {
                        Vec::new()
                    } else {
                        std::slice::from_raw_parts(bytes, len).to_vec()
                    };
                    let destination = if kind == abi::SEND {
                        Some(address(*addr)?)
                    } else {
                        None
                    };
                    E::with_driver(|d| {
                        if let Some(to) = destination {
                            d.send_to(handle, WriteBuf::Owned(data), to, TOKEN)
                        } else {
                            d.write(handle, WriteBuf::Owned(data), TOKEN)
                        }
                    })?
                }
                abi::ACCEPT => {
                    let handle = resource(res)?;
                    E::with_driver(|d| d.accept(handle, TOKEN))?
                }
                abi::SHUTDOWN => {
                    let handle = resource(res)?;
                    E::with_driver(|d| d.shutdown(handle, TOKEN))?
                }
                _ => return Err(invalid()),
            };
            *out = native_operation(id, buffer, timer);
            Ok(())
        })()
    })
}
unsafe fn poll_operations<E: Embedding>(
    ops: &[abi::Opaque],
    timeout: Timeout,
) -> turnloop::Result<()> {
    let mut ids = Vec::new();
    for &p in ops {
        if p.is_null() {
            return Err(invalid());
        }
        // SAFETY: Ops poll borrows live operations for this call.
        let operation = unsafe { &*p.cast::<NativeOperation>() };
        if operation.completion.is_none() && !ids.contains(&operation.id) {
            ids.push(operation.id);
        }
    }
    if ids.is_empty() {
        return Ok(());
    }
    for completion in E::turn_and_take(&ids, &[], timeout)? {
        let id = completion.op.ok_or_else(invalid)?;
        let pointer = ops
            .iter()
            .copied()
            .find(|&p| {
                // SAFETY: these live operation pointers were validated above.
                unsafe { (&*p.cast::<NativeOperation>()).id == id }
            })
            .ok_or_else(invalid)?;
        // SAFETY: the embedding returns at most one terminal completion per
        // selected one-shot operation. No operation is aliased across calls.
        let operation = unsafe { &mut *pointer.cast::<NativeOperation>() };
        if !completion.terminal || operation.completion.is_some() {
            return Err(invalid());
        }
        operation.completion = Some(completion);
    }
    Ok(())
}
unsafe extern "C" fn poll<E: Embedding>(
    operations: *const abi::Opaque,
    len: usize,
    block: bool,
) -> abi::Error {
    if len == 0 {
        return error(invalid());
    }
    // SAFETY: poll receives a valid borrowed pointer array from the host.
    let operations = unsafe { std::slice::from_raw_parts(operations, len) };
    let already_ready = operations.iter().any(|&p| {
        // SAFETY: every entry is an unconsumed operation from this table.
        unsafe { !p.is_null() && (&*p.cast::<NativeOperation>()).completion.is_some() }
    });
    let timeout = if block && !already_ready {
        Timeout::Forever
    } else {
        Timeout::Now
    };
    // SAFETY: the input array is borrowed only for this turn.
    status(unsafe { poll_operations::<E>(operations, timeout) })
}
unsafe extern "C" fn ready(operation: abi::Opaque) -> bool {
    // SAFETY: the host retains its operation until take/drop.
    unsafe { (&*operation.cast::<NativeOperation>()).completion.is_some() }
}
fn close<E: Embedding>(handle: Handle) -> turnloop::Result<()> {
    E::with_driver(|d| d.close(handle, TOKEN))?;
    loop {
        let completed = E::turn_and_take(&[], &[handle], Timeout::Forever)?;
        if completed
            .iter()
            .any(|c| c.handle == Some(handle) && matches!(c.result, OpResult::Closed))
        {
            return Ok(());
        }
    }
}
fn bytes_out(bytes: Vec<u8>) -> Buffer {
    let bytes = bytes.into_boxed_slice();
    let len = bytes.len();
    Buffer {
        data: Box::into_raw(bytes).cast(),
        len,
    }
}
unsafe extern "C" fn take<E: Embedding>(
    pointer: abi::Opaque,
    out: *mut abi::Completion,
) -> abi::Error {
    // SAFETY: the host passes an unconsumed operation and writable result.
    status(unsafe {
        (|| {
            let operation = &mut *pointer.cast::<NativeOperation>();
            if operation.completion.is_none() {
                return Err(Error::new(ErrorKind::WouldBlock));
            }
            // Close the timer first: failure leaves the operation and completion owned.
            if let Some(timer) = operation.timer {
                close::<E>(timer)?;
                operation.timer = None;
            }
            let mut operation = Box::from_raw(pointer.cast::<NativeOperation>());
            let result = operation
                .completion
                .take()
                .expect("checked completion")
                .result;
            let mut value = abi::Completion {
                kind: abi::COMPLETE,
                ..Default::default()
            };
            match result {
                OpResult::Err(e) => value.error = error(e),
                OpResult::Cancelled => value.error = error(Error::new(ErrorKind::Cancelled)),
                OpResult::Eof => value.kind = abi::EOF,
                OpResult::Read { n, .. } => {
                    operation.buffer.truncate(n);
                    value.transferred = n;
                    value.bytes = bytes_out(std::mem::take(&mut operation.buffer));
                }
                OpResult::RecvFrom { n, from, .. } => {
                    operation.buffer.truncate(n);
                    value.transferred = n;
                    value.address = encode_address(from);
                    value.bytes = bytes_out(std::mem::take(&mut operation.buffer));
                }
                OpResult::Wrote(n) => value.transferred = n,
                OpResult::Accepted { conn, peer } => {
                    value.kind = abi::ACCEPTED;
                    value.address = encode_address(peer);
                    value.resource = native_resource(conn);
                }
                OpResult::Resolved(addresses) => {
                    value.kind = abi::RESOLVED;
                    let addresses = addresses
                        .into_iter()
                        .map(encode_address)
                        .collect::<Vec<_>>()
                        .into_boxed_slice();
                    value.addresses.len = addresses.len();
                    value.addresses.data = Box::into_raw(addresses).cast();
                }
                OpResult::Connected | OpResult::Shutdown | OpResult::Timer => {}
                _ => value.error = error(invalid()),
            }
            *out = value;
            Ok(())
        })()
    })
}
unsafe extern "C" fn operation_drop<E: Embedding>(pointer: abi::Opaque) {
    // SAFETY: an operation is transferred exactly once. If the driver has failed,
    // retain the native allocation rather than free storage still used by the OS.
    let done = (|| -> turnloop::Result<()> {
        // SAFETY: short borrows never overlap a turn mutating this operation.
        let (id, pending) = unsafe {
            let op = &*pointer.cast::<NativeOperation>();
            (op.id, op.completion.is_none())
        };
        if pending {
            E::with_driver(|d| {
                d.cancel(id);
                Ok(())
            })?;
            // SAFETY: this operation remains owned until the final Box destruction.
            while !unsafe { ready(pointer) } {
                // SAFETY: buffer storage stays allocated until terminal acknowledgement.
                unsafe {
                    poll_operations::<E>(&[pointer], Timeout::Forever)?;
                }
            }
        }
        // SAFETY: all native buffer accesses ended; no other caller owns this operation.
        let operation = unsafe { &mut *pointer.cast::<NativeOperation>() };
        if let Some(timer) = operation.timer {
            close::<E>(timer)?;
            operation.timer = None;
        }
        if let Some(Completion {
            result: OpResult::Accepted { conn, .. },
            ..
        }) = &operation.completion
        {
            close::<E>(*conn)?;
        }
        Ok(())
    })();
    if done.is_ok() {
        // SAFETY: terminal acknowledgement ended every native access.
        drop(unsafe { Box::from_raw(pointer.cast::<NativeOperation>()) });
    }
}
unsafe extern "C" fn resource_drop<E: Embedding>(pointer: abi::Opaque) {
    // SAFETY: a native resource is transferred to its matching table once.
    let handle = unsafe { (&*pointer.cast::<NativeResource>()).0 };
    if close::<E>(handle).is_ok() {
        // SAFETY: Closed confirms release of the exact resource.
        drop(unsafe { Box::from_raw(pointer.cast::<NativeResource>()) });
    }
}
unsafe extern "C" fn completion_drop<E: Embedding>(value: abi::Completion) {
    if !value.resource.is_null() {
        // SAFETY: an unclaimed accepted resource belongs to this completion.
        unsafe {
            resource_drop::<E>(value.resource);
        }
    }
    if !value.bytes.data.is_null() {
        // SAFETY: bytes_out allocated exactly this boxed slice in this allocator.
        drop(unsafe {
            Box::from_raw(ptr::slice_from_raw_parts_mut(
                value.bytes.data,
                value.bytes.len,
            ))
        });
    }
    if !value.addresses.data.is_null() {
        // SAFETY: take allocated exactly this boxed address slice.
        drop(unsafe {
            Box::from_raw(ptr::slice_from_raw_parts_mut(
                value.addresses.data,
                value.addresses.len,
            ))
        });
    }
}
unsafe extern "C" fn raw_socket<E: Embedding>(p: abi::Opaque, raw: *mut usize) -> abi::Error {
    // SAFETY: the host borrows a live resource and a writable output.
    status(unsafe {
        (|| {
            let h = resource(p)?;
            *raw = E::with_driver(|d| match d.raw_transport(h)? {
                #[cfg(unix)]
                turnloop::RawTransport::Fd(fd) => Ok(fd as usize),
                #[cfg(windows)]
                turnloop::RawTransport::Socket(socket) => Ok(socket),
                #[allow(unreachable_patterns)]
                _ => Err(invalid()),
            })?;
            Ok(())
        })()
    })
}
unsafe extern "C" fn release_socket<E: Embedding>(p: abi::Opaque, raw: *mut usize) -> abi::Error {
    // SAFETY: the caller exclusively owns this resource, with no outstanding
    // operation retaining it. Driver::detach verifies quiescence independently.
    status(unsafe {
        (|| {
            let h = resource(p)?;
            let socket = E::with_driver(|d| d.detach(h))?;
            #[cfg(unix)]
            {
                use std::os::fd::IntoRawFd;
                *raw = socket.into_fd().into_raw_fd() as usize;
            }
            #[cfg(windows)]
            {
                use std::os::windows::io::IntoRawSocket;
                *raw = socket.into_socket()?.into_raw_socket() as usize;
            }
            drop(Box::from_raw(p.cast::<NativeResource>()));
            Ok(())
        })()
    })
}
unsafe extern "C" fn option<E: Embedding>(
    p: abi::Opaque,
    tag: u32,
    set: bool,
    values: *mut [u64; 4],
) -> abi::Error {
    use turnloop::{SocketOption as O, SocketOptionKind as K};
    // SAFETY: the caller borrows a live resource and a four-word value array.
    status(unsafe {
        (|| {
            let handle = resource(p)?;
            let v = &mut *values;
            E::with_driver(|d| {
                if tag == abi::SHUTDOWN_READ && set {
                    return d.shutdown_read(handle);
                }
                if tag == abi::LISTEN_BACKLOG && set {
                    return d
                        .set_listen_backlog(handle, u32::try_from(v[0]).map_err(|_| invalid())?);
                }
                if tag == abi::KEEPALIVE_PARAMETERS {
                    if set {
                        return d.set_keepalive_parameters(
                            handle,
                            turnloop::KeepAlive {
                                idle: (v[0] & 1 != 0).then(|| Duration::from_nanos(v[1])),
                                interval: (v[0] & 2 != 0).then(|| Duration::from_nanos(v[2])),
                                count: if v[0] & 4 != 0 {
                                    Some(u32::try_from(v[3]).map_err(|_| invalid())?)
                                } else {
                                    None
                                },
                            },
                        );
                    }
                    let k = d.keepalive_parameters(handle)?;
                    *v = [
                        7,
                        k.idle
                            .map_or(0, |d| d.as_nanos().min(u64::MAX as u128) as u64),
                        k.interval
                            .map_or(0, |d| d.as_nanos().min(u64::MAX as u128) as u64),
                        k.count.unwrap_or(0) as u64,
                    ];
                    return Ok(());
                }
                if set {
                    let option = match tag {
                        abi::KEEPALIVE => O::KeepAlive(if v[0] == 0 {
                            None
                        } else {
                            Some(turnloop::KeepAlive::default())
                        }),
                        abi::RECEIVE_BUFFER => {
                            O::RecvBufferSize(u32::try_from(v[0]).map_err(|_| invalid())?)
                        }
                        abi::SEND_BUFFER => {
                            O::SendBufferSize(u32::try_from(v[0]).map_err(|_| invalid())?)
                        }
                        abi::HOP_LIMIT => O::Ttl(u32::try_from(v[0]).map_err(|_| invalid())?),
                        _ => return Err(invalid()),
                    };
                    d.set_option(handle, option)
                } else {
                    let kind = match tag {
                        abi::KEEPALIVE => K::KeepAlive,
                        abi::RECEIVE_BUFFER => K::RecvBufferSize,
                        abi::SEND_BUFFER => K::SendBufferSize,
                        abi::HOP_LIMIT => K::Ttl,
                        _ => return Err(invalid()),
                    };
                    *v = [0; 4];
                    match d.get_option(handle, kind)? {
                        O::KeepAlive(Some(k)) => {
                            *v = [
                                1,
                                k.idle
                                    .map_or(0, |v| v.as_nanos().min(u64::MAX as u128) as u64),
                                k.interval
                                    .map_or(0, |v| v.as_nanos().min(u64::MAX as u128) as u64),
                                k.count.unwrap_or(0) as u64,
                            ];
                        }
                        O::KeepAlive(None) => {}
                        O::RecvBufferSize(n) | O::SendBufferSize(n) | O::Ttl(n) => v[0] = n as u64,
                        _ => return Err(invalid()),
                    }
                    Ok(())
                }
            })
        })()
    })
}

#[cfg(any(feature = "standalone", test))]
#[path = "standalone.rs"]
pub mod standalone;

#[cfg(test)]
#[path = "embedding_tests.rs"]
mod tests;
