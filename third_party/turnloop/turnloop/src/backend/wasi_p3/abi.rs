//! Hand lowering of asynchronous socket/clock imports, matching wasip3 0.8.0.
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6};
#[link(wasm_import_module = "wasi:clocks/monotonic-clock@0.3.0")]
unsafe extern "C" {
    #[link_name = "[async-lower]wait-until"]
    pub(super) fn deadline(at: u64) -> u32;
}
#[link(wasm_import_module = "wasi:sockets/types@0.3.0")]
unsafe extern "C" {
    #[link_name = "[async-lower][method]tcp-socket.connect"]
    fn raw_connect(params: *mut u32, result: *mut u32) -> u32;
    #[link_name = "[async-lower][method]udp-socket.send"]
    fn raw_send(params: *mut u32, result: *mut u32) -> u32;
    #[link_name = "[async-lower][method]udp-socket.receive"]
    fn raw_receive(socket: u32, result: *mut u32) -> u32;
}
pub fn encode_addr(storage: &mut [u32], addr: SocketAddr) {
    storage.fill(0);
    // SAFETY: exclusive initialized u32 storage, at least 32 bytes, viewed as bytes.
    let bytes = unsafe { std::slice::from_raw_parts_mut(storage.as_mut_ptr().cast::<u8>(), 32) };
    bytes[4..6].copy_from_slice(&addr.port().to_le_bytes());
    match addr {
        SocketAddr::V4(a) => bytes[6..10].copy_from_slice(&a.ip().octets()),
        SocketAddr::V6(a) => {
            bytes[0] = 1;
            bytes[8..12].copy_from_slice(&a.flowinfo().to_le_bytes());
            bytes[28..32].copy_from_slice(&a.scope_id().to_le_bytes());
            for (i, s) in a.ip().segments().into_iter().enumerate() {
                bytes[12 + i * 2..14 + i * 2].copy_from_slice(&s.to_le_bytes());
            }
        }
    }
}
pub fn decode_addr(storage: &[u32]) -> SocketAddr {
    // SAFETY: caller supplies the initialized 32-byte canonical address.
    let b = unsafe { std::slice::from_raw_parts(storage.as_ptr().cast::<u8>(), 32) };
    let port = u16::from_le_bytes([b[4], b[5]]);
    if b[0] == 0 {
        (Ipv4Addr::new(b[6], b[7], b[8], b[9]), port).into()
    } else {
        let segments = std::array::from_fn(|i| u16::from_le_bytes([b[12 + i * 2], b[13 + i * 2]]));
        SocketAddr::V6(SocketAddrV6::new(
            Ipv6Addr::from(segments),
            port,
            u32::from_le_bytes(b[8..12].try_into().expect("flow")),
            u32::from_le_bytes(b[28..32].try_into().expect("scope")),
        ))
    }
}

// Rust's wasm32-wasip3 shadow stack lives in canonical context slot 0.
// Synchronous completion of an async-lowered call may reset that slot. Preserve
// the caller's stack before subsequent Rust code touches a stack frame.
#[link(wasm_import_module = "env")]
unsafe extern "C" {
    fn __wasm_get_stack_pointer() -> u32;
    fn __wasm_set_stack_pointer(pointer: u32);
}
pub(super) fn preserving_stack<T>(f: impl FnOnce() -> T) -> T {
    // SAFETY: pinned wasm32-wasip3 toolchain's shadow-stack accessors; the saved
    // value identifies this still-live caller frame, never another task's stack.
    let stack = unsafe { __wasm_get_stack_pointer() };
    super::return_storage::scoped(|| {
        let result = f();
        // SAFETY: f has returned synchronously; restore this caller's live stack
        // before the canonical allocation scope runs its Rust destructors.
        unsafe { __wasm_set_stack_pointer(stack) };
        result
    })
}
pub(super) unsafe fn connect(params: *mut u32, result: *mut u32) -> u32 {
    // SAFETY: caller owns pinned canonical parameters/return area until acknowledgement.
    preserving_stack(|| unsafe { raw_connect(params, result) })
}
pub(super) unsafe fn send(params: *mut u32, result: *mut u32) -> u32 {
    // SAFETY: caller owns pinned canonical parameters/return area until acknowledgement.
    preserving_stack(|| unsafe { raw_send(params, result) })
}
pub(super) unsafe fn receive(socket: u32, result: *mut u32) -> u32 {
    // SAFETY: caller owns socket and pinned return area until acknowledgement.
    preserving_stack(|| unsafe { raw_receive(socket, result) })
}
