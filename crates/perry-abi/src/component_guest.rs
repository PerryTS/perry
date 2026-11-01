//! Canonical guest adapters exchange rooted slot indices with the runtime.
//! A slot is local to one frame; it is never a Perry heap address.
use core::ffi::c_void;

pub const GUEST_ABI_VERSION: u32 = 2;

pub type GuestImport =
    unsafe extern "C" fn(frame: *mut c_void, interface: u32, function: u32, args: u32) -> u32;

/// Canonical resource operations: 0 creates an exported resource from a stable
/// representation ID, 1 drops an owned canonical handle. No heap pointers cross.
pub type GuestResource =
    unsafe extern "C" fn(frame: *mut c_void, resource: u32, operation: u32, value: u32) -> u32;

#[repr(C)]
pub struct ComponentGuestOps {
    pub abi_version: u32,
    pub size: usize,
    /// Immutable manifest bytes and callbacks in the linked adapter image.
    pub contract: *const u8,
    pub contract_len: usize,
    pub import: GuestImport,
    pub resource: GuestResource,
}

/// Additive endpoint-operation table; the published Guest ABI 2 is unchanged.
/// Operation 0 starts a read with `argument` as the positive element capacity.
/// Return 0 only after adopting the fresh heap frame; -1 leaves it with Rust.
/// The frame contains rooted private operation state, never a raw JS address.
pub type GuestEndpointOperation = unsafe extern "C" fn(
    frame: *mut c_void,
    ty: u32,
    handle: u32,
    operation: u32,
    argument: u32,
) -> i32;
pub const GUEST_ENDPOINT_ABI_VERSION: u32 = 1;
#[repr(C)]
pub struct ComponentGuestEndpointOps {
    pub abi_version: u32,
    pub size: usize,
    pub operation: GuestEndpointOperation,
}
