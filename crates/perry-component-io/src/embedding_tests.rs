use super::*;
use standalone::Standalone;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream, UdpSocket},
};
fn ops() -> &'static abi::Ops {
    standalone::ops()
}
fn check(e: abi::Error) {
    assert_eq!((e.kind, e.os), (0, 0));
}
#[cfg(unix)]
fn raw<T: std::os::fd::IntoRawFd>(socket: T) -> usize {
    socket.into_raw_fd() as usize
}
#[cfg(windows)]
fn raw<T: std::os::windows::io::IntoRawSocket>(socket: T) -> usize {
    socket.into_raw_socket() as usize
}
unsafe fn adopt_test(socket: usize) -> abi::Opaque {
    let mut res = ptr::null_mut();
    let mut op = ptr::null_mut();
    // SAFETY: transfer the owned socket, writable outputs, no connect destination.
    check(unsafe { (ops().adopt)(socket, false, ptr::null(), &mut res, &mut op) });
    assert!(op.is_null());
    res
}
unsafe fn submit_test(
    res: abi::Opaque,
    kind: u32,
    bytes: &[u8],
    length: usize,
    address: Option<abi::Address>,
    delay: u64,
) -> abi::Opaque {
    let mut op = ptr::null_mut();
    let addr = address.as_ref().map_or(ptr::null(), ptr::from_ref);
    // SAFETY: this test owns res; submit copies bytes or allocates its read buffer.
    check(unsafe { (ops().submit)(res, kind, bytes.as_ptr(), length, addr, delay, &mut op) });
    op
}
unsafe fn wait(op: abi::Opaque) -> abi::Completion {
    // SAFETY: timer submission needs no socket resource or input buffer.
    let deadline = unsafe { submit_test(ptr::null_mut(), abi::TIMER, &[], 0, None, 5_000_000_000) };
    let selected = [op, deadline];
    // SAFETY: both operations remain owned throughout this wait.
    unsafe {
        while !(ops().ready)(op) {
            check((ops().poll)(selected.as_ptr(), selected.len(), true));
            assert!(
                !(ops().ready)(deadline),
                "native operation exceeded five seconds"
            );
        }
        (ops().operation_drop)(deadline);
        let mut complete = abi::Completion::default();
        check((ops().take)(op, &mut complete));
        complete
    }
}
fn no_remaining_operations() {
    assert!(!Standalone::with_driver(|d| Ok(d.alive())).unwrap());
}
#[test]
fn timer_poll_selects_only_requested_operations() {
    // SAFETY: every pointer originates from this table and is consumed once.
    unsafe {
        let a = submit_test(ptr::null_mut(), abi::TIMER, &[], 0, None, 0);
        let b = submit_test(ptr::null_mut(), abi::TIMER, &[], 0, None, 0);
        let result = wait(b);
        assert_eq!(result.kind, abi::COMPLETE);
        (ops().completion_drop)(result);
        assert!(!(ops().ready)(a), "unselected operation must remain staged");
        let result = wait(a);
        (ops().completion_drop)(result);
    }
    no_remaining_operations();
}
#[test]
fn tcp_buffers_and_cancel_acknowledgement_round_trip() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (mut peer, _) = listener.accept().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    // SAFETY: this scope owns the native socket and each returned operation/result.
    unsafe {
        let resource = adopt_test(raw(client));
        let read = submit_test(resource, abi::READ, &[], 64, None, 0);
        peer.write_all(b"hello").unwrap();
        let result = wait(read);
        check(result.error);
        assert_eq!(
            std::slice::from_raw_parts(result.bytes.data, result.bytes.len),
            b"hello"
        );
        (ops().completion_drop)(result);
        let write = submit_test(resource, abi::WRITE, b"world", 5, None, 0);
        let result = wait(write);
        check(result.error);
        assert_eq!(result.transferred, 5);
        (ops().completion_drop)(result);
        let mut bytes = [0; 5];
        peer.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"world");
        let pending = submit_test(resource, abi::READ, &[], 65536, None, 0);
        (ops().operation_drop)(pending);
        (ops().resource_drop)(resource);
        assert_eq!(peer.read(&mut bytes).unwrap(), 0);
    }
    no_remaining_operations();
}
#[test]
fn udp_retains_datagram_source_and_bound_endpoint() {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let bound = socket.local_addr().unwrap();
    let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    // SAFETY: transfer socket ownership and consume each operation once.
    unsafe {
        let resource = adopt_test(raw(socket));
        let receive = submit_test(resource, abi::RECEIVE, &[], 65536, None, 0);
        peer.send_to(b"packet", bound).unwrap();
        let result = wait(receive);
        check(result.error);
        assert_eq!(address(result.address).unwrap(), peer.local_addr().unwrap());
        assert_eq!(
            std::slice::from_raw_parts(result.bytes.data, result.bytes.len),
            b"packet"
        );
        (ops().completion_drop)(result);
        let send = submit_test(
            resource,
            abi::SEND,
            b"reply",
            5,
            Some(encode_address(peer.local_addr().unwrap())),
            0,
        );
        let result = wait(send);
        check(result.error);
        (ops().completion_drop)(result);
        let mut bytes = [0; 16];
        let (n, source) = peer.recv_from(&mut bytes).unwrap();
        assert_eq!(&bytes[..n], b"reply");
        assert_eq!(source, bound);
        (ops().resource_drop)(resource);
    }
    no_remaining_operations();
}
#[test]
fn dns_results_are_owned_by_the_producing_table() {
    // SAFETY: resolution copies the name and returns an owned result array.
    unsafe {
        let op = submit_test(ptr::null_mut(), abi::RESOLVE, b"localhost", 9, None, 0);
        let result = wait(op);
        check(result.error);
        assert_eq!(result.kind, abi::RESOLVED);
        assert!(result.addresses.len > 0);
        for addr in std::slice::from_raw_parts(result.addresses.data, result.addresses.len) {
            assert!(address(*addr).unwrap().ip().is_loopback());
        }
        (ops().completion_drop)(result);
    }
    no_remaining_operations();
}
