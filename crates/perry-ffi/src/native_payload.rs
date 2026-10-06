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

impl PayloadFamily {
    /// One static descriptor for one Rust payload type and vtable.
    pub const fn new<T>(
        class_id: u32,
        name: &'static str,
        links_owner: bool,
        vtable: &'static crate::native_stream::PayloadVTable,
    ) -> Self {
        Self {
            abi: layout_digest(),
            class_id,
            links_owner: links_owner as u32,
            constructor_length: 1,
            _reserved: 0,
            name: name.as_ptr(),
            name_len: name.len(),
            install_prototype: None,
            vtable,
            payload_size: std::mem::size_of::<T>(),
            payload_align: std::mem::align_of::<T>(),
        }
    }
}

extern "C" {
    fn js_perry_payload_alloc(
        family: *const PayloadFamily,
        resource: *mut c_void,
        proto: f64,
        bytes: usize,
    ) -> f64;
    fn js_perry_payload_attach(
        value: f64,
        family: *const PayloadFamily,
        resource: *mut c_void,
        bytes: usize,
    ) -> i32;
    fn js_perry_payload_get(value: f64, family: *const PayloadFamily) -> *mut c_void;
    fn js_perry_payload_close(value: f64, family: *const PayloadFamily) -> i32;
}

/// # Safety
/// T must be the descriptor's payload type. Ownership transfers on success.
pub unsafe fn alloc<T>(
    family: &'static PayloadFamily,
    payload: T,
    proto: f64,
    bytes: usize,
) -> f64 {
    assert!(abi_matches(), "native payload ABI mismatch");
    let ptr = Box::into_raw(Box::new(payload));
    let value = js_perry_payload_alloc(family, ptr.cast(), proto, bytes);
    if value.to_bits() == crate::JsValue::UNDEFINED.bits() {
        drop(Box::from_raw(ptr));
    }
    value
}

/// # Safety
/// T must be the descriptor's payload type.
pub unsafe fn attach<T>(
    value: f64,
    family: &'static PayloadFamily,
    payload: T,
    bytes: usize,
) -> bool {
    assert!(abi_matches(), "native payload ABI mismatch");
    let ptr = Box::into_raw(Box::new(payload));
    if js_perry_payload_attach(value, family, ptr.cast(), bytes) == 0 {
        true
    } else {
        drop(Box::from_raw(ptr));
        false
    }
}

/// # Safety
/// T is the descriptor's type. No allocation or JS while the reference lives.
pub unsafe fn get_mut<'a, T>(value: f64, family: &'static PayloadFamily) -> Option<&'a mut T> {
    assert!(abi_matches(), "native payload ABI mismatch");
    (js_perry_payload_get(value, family) as *mut T).as_mut()
}

/// Close synchronously, without reopening the cell.
pub fn close(value: f64, family: &'static PayloadFamily) -> bool {
    assert!(abi_matches(), "native payload ABI mismatch");
    unsafe { js_perry_payload_close(value, family) == 0 }
}

extern "C" {
    fn js_perry_payload_prototype(
        family: *const PayloadFamily,
        module: *const u8,
        module_len: usize,
    ) -> f64;
    fn js_perry_payload_proto_method(
        proto: *mut c_void,
        name: *const u8,
        name_len: usize,
        info: *const crate::JsFunctionInfo,
        arity: u32,
    );
    fn js_perry_payload_own(owner: f64, key: *const u8, key_len: usize, value: f64);
}
impl PayloadFamily {
    /// Add a prototype installer to a static descriptor.
    pub const fn with_installer(mut self, installer: unsafe extern "C" fn(*mut c_void)) -> Self {
        self.install_prototype = Some(installer);
        self
    }
}
/// The canonical constructor prototype; install methods only on first adoption.
pub fn prototype(family: &'static PayloadFamily, module: &str) -> f64 {
    assert!(abi_matches(), "native payload ABI mismatch");
    unsafe { js_perry_payload_prototype(family, module.as_ptr(), module.len()) }
}
/// # Safety
/// proto is the live object handed to a descriptor's installer.
pub unsafe fn prototype_method(
    proto: *mut c_void,
    name: &str,
    info: &'static crate::JsFunctionInfo,
    arity: u32,
) {
    js_perry_payload_proto_method(proto, name.as_ptr(), name.len(), info, arity);
}
/// Define an ordinary own field.
pub fn own(owner: f64, key: &str, value: f64) {
    unsafe { js_perry_payload_own(owner, key.as_ptr(), key.len(), value) };
}

extern "C" {
    fn js_perry_payload_get_attached(
        value: f64,
        vtable: *const crate::native_stream::PayloadVTable,
    ) -> *mut c_void;
    fn js_perry_payload_close_attached(
        value: f64,
        vtable: *const crate::native_stream::PayloadVTable,
    ) -> i32;
}
/// Access the attached payload by its type vtable, including subclasses and
/// distinct constructor families that share one Rust payload type.
/// # Safety
/// T is the vtable's payload type; no allocation or JS while borrowed.
pub unsafe fn get_attached<'a, T>(
    value: f64,
    vtable: &'static crate::native_stream::PayloadVTable,
) -> Option<&'a mut T> {
    assert!(abi_matches(), "native payload ABI mismatch");
    (js_perry_payload_get_attached(value, vtable) as *mut T).as_mut()
}
/// # Safety
/// vtable names the attached payload's static type.
pub unsafe fn close_attached(
    value: f64,
    vtable: &'static crate::native_stream::PayloadVTable,
) -> bool {
    assert!(abi_matches(), "native payload ABI mismatch");
    js_perry_payload_close_attached(value, vtable) == 0
}

extern "C" {
    fn js_perry_payload_external_bytes(
        value: f64,
        vtable: *const crate::native_stream::PayloadVTable,
        bytes: usize,
    );
}
/// Restate native bytes after a non-stream codec mutation, e.g. reset/params.
pub fn set_external_bytes(
    value: f64,
    vtable: &'static crate::native_stream::PayloadVTable,
    bytes: usize,
) {
    unsafe { js_perry_payload_external_bytes(value, vtable, bytes) };
}
