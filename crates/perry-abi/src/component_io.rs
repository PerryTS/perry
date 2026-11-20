//! Synchronous, versioned native I/O boundary for optional component hosts.
//! Opaque pointers own real resources or operations in the supplying allocator.
//! Calls, including destruction, stay on the owning agent thread. Poll never
//! dispatches callbacks. An operation must be taken or dropped exactly once.
use super::component_host::Buffer;
use core::ffi::c_void;

pub const ABI_VERSION: u32 = 1;
pub type Opaque = *mut c_void;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Address {
    pub octets: [u8; 16],
    pub port: u16,
    pub family: u16,
    pub flow_info: u32,
    pub scope_id: u32,
}
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Error {
    /// Zero means success. Other values are the ERR_* constants below.
    pub kind: u32,
    /// Native OS error, or zero if the driver supplied only a portable category.
    pub os: i32,
}
pub const ERR_CANCELLED: u32 = 1;
pub const ERR_UNSUPPORTED: u32 = 2;
pub const ERR_INVALID: u32 = 3;
pub const ERR_NOT_FOUND: u32 = 4;
pub const ERR_WOULD_BLOCK: u32 = 5;
pub const ERR_TIMEOUT: u32 = 6;
pub const ERR_REFUSED: u32 = 7;
pub const ERR_RESET: u32 = 8;
pub const ERR_PIPE: u32 = 9;
pub const ERR_LIMIT: u32 = 10;
pub const ERR_ACCESS: u32 = 11;
pub const ERR_OTHER: u32 = 12;

pub const READ: u32 = 1;
pub const WRITE: u32 = 2;
pub const RECEIVE: u32 = 3;
pub const SEND: u32 = 4;
pub const ACCEPT: u32 = 5;
pub const SHUTDOWN: u32 = 6;
pub const TIMER: u32 = 7;
pub const RESOLVE: u32 = 8;

pub const COMPLETE: u32 = 1;
pub const EOF: u32 = 2;
pub const ACCEPTED: u32 = 3;
pub const RESOLVED: u32 = 4;

#[repr(C)]
#[derive(Default)]
pub struct AddressBuffer {
    pub data: *mut Address,
    pub len: usize,
}
#[repr(C)]
#[derive(Default)]
pub struct Completion {
    pub kind: u32,
    pub error: Error,
    pub transferred: usize,
    pub address: Address,
    pub resource: Opaque,
    pub bytes: Buffer,
    pub addresses: AddressBuffer,
}

/// All pointers passed to this table remain in the calling host/runtime.
/// Buffers and completion-owned resources are released by this same table.
#[repr(C)]
pub struct Ops {
    pub abi_version: u32,
    pub size: usize,
    /// Consume an owned native socket even on error. A nonnull destination
    /// starts a bound TCP connect and also returns its operation.
    pub adopt: unsafe extern "C" fn(
        raw: usize,
        listener: bool,
        destination: *const Address,
        resource: *mut Opaque,
        operation: *mut Opaque,
    ) -> Error,
    /// Input bytes are copied before returning. READ/RECEIVE length requests
    /// owned native storage; TIMER delay is nanoseconds; RESOLVE bytes are UTF8.
    pub submit: unsafe extern "C" fn(
        resource: Opaque,
        kind: u32,
        bytes: *const u8,
        len: usize,
        address: *const Address,
        delay: u64,
        operation: *mut Opaque,
    ) -> Error,
    /// One driver turn and extraction of these exact operations only.
    pub poll: unsafe extern "C" fn(operations: *const Opaque, len: usize, block: bool) -> Error,
    pub ready: unsafe extern "C" fn(operation: Opaque) -> bool,
    /// Consumes a ready operation. WOULD_BLOCK leaves the operation owned.
    pub take: unsafe extern "C" fn(operation: Opaque, result: *mut Completion) -> Error,
    /// Cancel and await terminal acknowledgement before releasing storage.
    pub operation_drop: unsafe extern "C" fn(operation: Opaque),
    pub resource_drop: unsafe extern "C" fn(resource: Opaque),
    pub completion_drop: unsafe extern "C" fn(result: Completion),
    /// Read-only native identity for endpoint/option queries. Never transfer
    /// ownership, perform I/O, or alter driver registration through this value.
    pub raw_socket: unsafe extern "C" fn(resource: Opaque, raw: *mut usize) -> Error,
    /// Detach a quiescent socket. Success consumes the resource and transfers
    /// native ownership; failure leaves the resource owned by the caller.
    pub release_socket: unsafe extern "C" fn(resource: Opaque, raw: *mut usize) -> Error,
    /// Kernel-backed socket options. Tags and values defined below.
    pub option:
        unsafe extern "C" fn(resource: Opaque, tag: u32, set: bool, values: *mut [u64; 4]) -> Error,
}
pub const KEEPALIVE: u32 = 1;
pub const RECEIVE_BUFFER: u32 = 2;
pub const SEND_BUFFER: u32 = 3;
pub const HOP_LIMIT: u32 = 4;

pub const KEEPALIVE_PARAMETERS: u32 = 5;
pub const LISTEN_BACKLOG: u32 = 6;
pub const SHUTDOWN_READ: u32 = 7;
