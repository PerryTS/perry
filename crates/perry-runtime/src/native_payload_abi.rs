//! The C ABI of the native-payload pattern, for families that live in a
//! separately linked binding (#11919, NET-TRANSPORT-DESIGN P0).
//!
//! An ext crate (`perry-ext-net`, ...) links only `perry-ffi`, so it cannot
//! name `NativePayloadFamily` or call the generic `native_payload::alloc::<T>`.
//! It describes its family with a `#[repr(C)]` [`PerryPayloadFamily`] instead:
//! the class id, whether the cell links its owner, the constructor name, a
//! prototype installer, and the payload's size, alignment and vtable. The
//! payload itself is a `Box<T>` the binding allocates and hands over as a raw
//! pointer; from then on it is an ordinary payload cell, with every rule of
//! `docs/native-payload-pattern.md`.
//!
//! Every entry point here forwards to the same code the in-tree families use
//! (`native_payload.rs`), so there is one implementation of the cell, its
//! lifecycle and its owner link. The descriptor carries a layout digest in its
//! first word ([`js_perry_payload_abi_layout`]); a descriptor written against
//! another layout is refused rather than misread.

use std::ffi::c_void;

/// Revision of this ABI. Bumped with any signature or descriptor change.
pub const PERRY_PAYLOAD_ABI_VERSION: u8 = 2;

/// A family, as a binding declares it. Lives in a `static` of the binding.
///
/// `abi` must stay the first field: it is read before any other field, so a
/// descriptor of a different layout is recognised before it is misread.
#[repr(C)]
pub struct PerryPayloadFamily {
    /// [`js_perry_payload_abi_layout`] as the binding computed it.
    pub abi: u64,
    /// The family's id from `native_class_ids.rs`.
    pub class_id: u32,
    /// Nonzero: the cell keeps a traced edge to its owner (`owner_link`).
    pub links_owner: u32,
    /// `x.constructor.length` of the stand-in constructor.
    pub constructor_length: u32,
    /// Padding, so the layout does not depend on how a compiler packs it.
    pub _reserved: u32,
    /// The constructor name node reports, UTF-8, not NUL-terminated, static.
    pub name: *const u8,
    /// Length of `name`.
    pub name_len: usize,
    /// Installs the prototype's methods, once per realm. May be absent.
    pub install_prototype: Option<unsafe extern "C" fn(proto: *mut c_void)>,
    /// The payload type's static vtable: its `drop` frees one boxed payload
    /// (at release, sweep or thread teardown; it must not allocate on the GC
    /// heap, call JS or touch thread-locals) and its `stream` names the stream
    /// hooks of a stream family.
    pub vtable: *const crate::native_payload::PayloadVTable,
    /// `size_of::<T>()` of the payload.
    pub payload_size: usize,
    /// `align_of::<T>()` of the payload.
    pub payload_align: usize,
}

/// One own enumerable property set at allocation (node's own fields).
#[repr(C)]
pub struct PerryPayloadOwnProp {
    /// Property name, ASCII, not NUL-terminated.
    pub key: *const u8,
    /// Length of `key`.
    pub key_len: usize,
    /// NaN-boxed value; rooted by the runtime for the allocation.
    pub value: f64,
}

/// Digest of [`PerryPayloadFamily`]'s layout plus the ABI revision. perry-ffi
/// computes the same expression over its own copy of the struct.
#[no_mangle]
pub extern "C" fn js_perry_payload_abi_layout() -> u64 {
    payload_abi_layout()
}

pub(crate) const fn payload_abi_layout() -> u64 {
    use std::mem::{offset_of, size_of};
    (size_of::<PerryPayloadFamily>() as u64) << 48
        | (offset_of!(PerryPayloadFamily, class_id) as u64) << 40
        | (offset_of!(PerryPayloadFamily, name) as u64) << 32
        | (offset_of!(PerryPayloadFamily, install_prototype) as u64) << 24
        | (offset_of!(PerryPayloadFamily, vtable) as u64) << 16
        | (offset_of!(PerryPayloadFamily, payload_align) as u64) << 8
        | PERRY_PAYLOAD_ABI_VERSION as u64
}

// B1 adds a byte-span ABI beside P0's payload ABI, without changing P0's
// descriptor digest. Bindings check BOTH digests in abi_matches().
#[repr(C)]
#[derive(Default)]
pub struct PerryBytes {
    pub ptr: *mut u8,
    pub len: usize,
    pub pin: u64,
}

pub const fn bytes_abi_layout() -> u64 {
    use std::mem::{offset_of, size_of};
    (size_of::<PerryBytes>() as u64) << 48
        | (offset_of!(PerryBytes, len) as u64) << 32
        | (offset_of!(PerryBytes, pin) as u64) << 16
        | 1
}
#[no_mangle]
pub extern "C" fn js_perry_bytes_abi_layout() -> u64 {
    bytes_abi_layout()
}
fn byte_brand(brand: u32) -> Option<crate::buffer::bytes::Brand> {
    use crate::buffer::bytes::Brand;
    match brand {
        0 => Some(Brand::Buffer),
        1 => Some(Brand::Uint8Array),
        2 => Some(Brand::ArrayBuffer),
        3 => Some(Brand::DataView),
        _ => None,
    }
}
fn bytes_undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// # Safety
/// out is writable; the returned span expires at the next safepoint.
#[no_mangle]
pub unsafe extern "C" fn js_perry_bytes_borrow(value: f64, out: *mut PerryBytes) -> i32 {
    if out.is_null() {
        return -1;
    }
    out.write(PerryBytes::default());
    match crate::buffer::bytes::span(value, false) {
        Ok(span) => {
            out.write(PerryBytes {
                ptr: span.ptr,
                len: span.len,
                pin: 0,
            });
            0
        }
        Err(_) => -1,
    }
}
/// # Safety
/// out is writable. Release its ticket exactly once, on this thread.
#[no_mangle]
pub unsafe extern "C" fn js_perry_bytes_pin(value: f64, out: *mut PerryBytes) -> i32 {
    if out.is_null() {
        return -1;
    }
    out.write(PerryBytes::default());
    match crate::buffer::bytes::pin(value) {
        Ok(pin) => {
            let ticket = Box::new(pin);
            out.write(PerryBytes {
                ptr: ticket.ptr,
                len: ticket.len,
                pin: Box::into_raw(ticket) as u64,
            });
            0
        }
        Err(_) => -1,
    }
}
/// # Safety
/// ticket is zero or an unreleased pin ticket from this thread.
#[no_mangle]
pub unsafe extern "C" fn js_perry_bytes_unpin(ticket: u64) {
    if ticket != 0 {
        drop(Box::from_raw(ticket as *mut crate::buffer::bytes::Pinned));
    }
}
/// # Safety
/// out is writable. ZERO=0, UNINIT=1. A successful result owns a pin ticket.
#[no_mangle]
pub unsafe extern "C" fn js_perry_bytes_new(
    brand: u32,
    len: usize,
    init: u32,
    out: *mut PerryBytes,
) -> f64 {
    if out.is_null() {
        return bytes_undefined();
    }
    out.write(PerryBytes::default());
    let Some(brand) = byte_brand(brand) else {
        return bytes_undefined();
    };
    let init = match init {
        0 => crate::buffer::bytes::Init::Zero,
        1 => crate::buffer::bytes::Init::Uninit,
        _ => return bytes_undefined(),
    };
    let (value, pin) = crate::buffer::bytes::new_bytes(brand, len, init);
    out.write(PerryBytes {
        ptr: pin.ptr,
        len: pin.len,
        pin: Box::into_raw(Box::new(pin)) as u64,
    });
    value
}
/// # Safety
/// ptr is readable for len bytes (null is accepted only for an empty span).
#[no_mangle]
pub unsafe extern "C" fn js_perry_bytes_copy(brand: u32, ptr: *const u8, len: usize) -> f64 {
    let Some(brand) = byte_brand(brand) else {
        return bytes_undefined();
    };
    if ptr.is_null() && len != 0 {
        return bytes_undefined();
    }
    let input = if len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(ptr, len)
    };
    crate::buffer::bytes::from_slice(brand, input)
}
/// Consume external bytes. On today's inline allocation rule, consumption
/// copies then frees; B3 can adopt the allocation without changing consumers.
/// # Safety
/// ptr/len/cap belong to free, which must not call JS or allocate GC cells.
#[no_mangle]
pub unsafe extern "C" fn js_perry_bytes_adopt(
    brand: u32,
    ptr: *mut u8,
    len: usize,
    cap: usize,
    free: unsafe extern "C" fn(*mut u8, usize, usize),
) -> f64 {
    let result = if len <= cap {
        js_perry_bytes_copy(brand, ptr, len)
    } else {
        bytes_undefined()
    };
    free(ptr, len, cap);
    result
}

/// Debug no_gc scope boundary for separately linked bindings.
#[no_mangle]
pub extern "C" fn js_perry_bytes_scope_enter() {
    crate::buffer::bytes::scope_enter();
}
/// Match a scope boundary on its creating thread.
#[no_mangle]
pub extern "C" fn js_perry_bytes_scope_leave() {
    crate::buffer::bytes::scope_leave();
}

// External archives reference these by name. Preserve them in the pruned
// runtime exactly as the existing byte accessor is preserved.
#[cfg(feature = "keepalive-anchors")]
mod byte_keepalive {
    use super::*;
    #[used(compiler)]
    static SCOPE_ENTER: extern "C" fn() = js_perry_bytes_scope_enter;
    #[used(compiler)]
    static SCOPE_LEAVE: extern "C" fn() = js_perry_bytes_scope_leave;
    #[used(compiler)]
    static BORROW: unsafe extern "C" fn(f64, *mut PerryBytes) -> i32 = js_perry_bytes_borrow;
    #[used(compiler)]
    static PIN: unsafe extern "C" fn(f64, *mut PerryBytes) -> i32 = js_perry_bytes_pin;
    #[used(compiler)]
    static UNPIN: unsafe extern "C" fn(u64) = js_perry_bytes_unpin;
    #[used(compiler)]
    static NEW: unsafe extern "C" fn(u32, usize, u32, *mut PerryBytes) -> f64 = js_perry_bytes_new;
    #[used(compiler)]
    static COPY: unsafe extern "C" fn(u32, *const u8, usize) -> f64 = js_perry_bytes_copy;
    #[used(compiler)]
    static ADOPT: unsafe extern "C" fn(
        u32,
        *mut u8,
        usize,
        usize,
        unsafe extern "C" fn(*mut u8, usize, usize),
    ) -> f64 = js_perry_bytes_adopt;
    #[used(compiler)]
    static BYTES_DIGEST: extern "C" fn() -> u64 = js_perry_bytes_abi_layout;
    #[used(compiler)]
    static PAYLOAD_DIGEST: extern "C" fn() -> u64 = js_perry_payload_abi_layout;
}
