//! Canonical guest adapters exchange rooted slot indices with the runtime.
//! A slot is local to one frame; it is never a Perry heap address.
use core::ffi::c_void;

pub const GUEST_ABI_VERSION: u32 = 1;

pub type GuestImport =
    unsafe extern "C" fn(frame: *mut c_void, interface: u32, function: u32, args: u32) -> u32;

#[repr(C)]
pub struct ComponentGuestOps {
    pub abi_version: u32,
    pub size: usize,
    /// Immutable manifest bytes and callbacks in the linked adapter image.
    pub contract: *const u8,
    pub contract_len: usize,
    pub import: GuestImport,
}
