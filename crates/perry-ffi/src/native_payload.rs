//! Native payloads for binding crates: an ordinary JS object that owns a
//! `Box<T>` of the binding's Rust state (#11919; the pattern and its rules are
//! `docs/native-payload-pattern.md` in the perry repo).
//!
//! The runtime's `native_payload` module is generic over `T`, which a
//! separately linked binding cannot instantiate. A binding declares its
//! family as a `static` [`PayloadFamily`] instead (class id, owner link,
//! constructor name, prototype installer, and the payload's size, alignment
//! and vtable, all derived from `T` by [`PayloadFamily::new`]) and calls
//! the functions here, which forward to the same runtime code the in-tree
//! families use: one cell, one lifecycle, one owner link.
//!
//! Rules for `T`, unchanged: plain Rust data only (no JS values, NaN-boxed
//! bits or GC pointers; keep JS values in [`js_state`]); `Drop for T` must not
//! allocate on the GC heap, call JS or touch thread-locals.

use std::ffi::c_void;

/// Revision of the payload ABI this file is written against.
const PAYLOAD_ABI_VERSION: u8 = 2;

/// A family, declared once per binding as a `static`.
///
/// Layout-checked against the runtime's copy: its first word is this crate's
/// layout digest, and the runtime refuses a descriptor whose digest differs.
#[repr(C)]
pub struct PayloadFamily {
    abi: u64,
    class_id: u32,
    links_owner: u32,
    constructor_length: u32,
    _reserved: u32,
    name: *const u8,
    name_len: usize,
    install_prototype: Option<unsafe extern "C" fn(proto: *mut c_void)>,
    vtable: *const crate::native_stream::PayloadVTable,
    payload_size: usize,
    payload_align: usize,
}

// SAFETY: every field is immutable static data (a name in `.rodata`, function
// pointers, integers); the descriptor is only ever read.
unsafe impl Sync for PayloadFamily {}

const fn layout_digest() -> u64 {
    use std::mem::{offset_of, size_of};
    (size_of::<PayloadFamily>() as u64) << 48
        | (offset_of!(PayloadFamily, class_id) as u64) << 40
        | (offset_of!(PayloadFamily, name) as u64) << 32
        | (offset_of!(PayloadFamily, install_prototype) as u64) << 24
        | (offset_of!(PayloadFamily, vtable) as u64) << 16
        | (offset_of!(PayloadFamily, payload_align) as u64) << 8
        | PAYLOAD_ABI_VERSION as u64
}

extern "C" {
    fn js_perry_payload_abi_layout() -> u64;
}
/// P0's descriptor digest and B1's byte-span digest must both match.
pub fn abi_matches() -> bool {
    unsafe { js_perry_payload_abi_layout() == layout_digest() && crate::bytes::abi_matches() }
}
