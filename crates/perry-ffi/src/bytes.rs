//! The lifetime-checked byte ABI. No binding knows the runtime's byte layout.
use crate::JsValue;
use std::marker::PhantomData;
#[repr(C)]
#[derive(Default)]
struct PerryBytes {
    ptr: *mut u8,
    len: usize,
    pin: u64,
}
const fn layout_digest() -> u64 {
    use std::mem::{offset_of, size_of};
    (size_of::<PerryBytes>() as u64) << 48
        | (offset_of!(PerryBytes, len) as u64) << 32
        | (offset_of!(PerryBytes, pin) as u64) << 16
        | 1
}
extern "C" {
    #[cfg(debug_assertions)]
    fn js_perry_bytes_scope_enter();
    #[cfg(debug_assertions)]
    fn js_perry_bytes_scope_leave();
    fn js_perry_bytes_abi_layout() -> u64;
    fn js_perry_bytes_borrow(value: f64, out: *mut PerryBytes) -> i32;
    fn js_perry_bytes_pin(value: f64, out: *mut PerryBytes) -> i32;
    fn js_perry_bytes_unpin(ticket: u64);
    fn js_perry_bytes_new(brand: u32, len: usize, init: u32, out: *mut PerryBytes) -> f64;
    fn js_perry_bytes_copy(brand: u32, ptr: *const u8, len: usize) -> f64;
    fn js_perry_bytes_adopt(
        brand: u32,
        ptr: *mut u8,
        len: usize,
        cap: usize,
        free: unsafe extern "C" fn(*mut u8, usize, usize),
    ) -> f64;
}
pub(crate) fn abi_matches() -> bool {
    unsafe { js_perry_bytes_abi_layout() == layout_digest() }
}
fn check_abi() {
    assert!(
        crate::native_payload::abi_matches(),
        "native byte ABI mismatch"
    );
}
/// Proof token for a scope without allocation, JavaScript, or safepoints.
pub struct NoGc<'s>(PhantomData<&'s mut &'s ()>);
/// Do not allocate, call JS, or poll while a borrow is held.
pub fn no_gc<R>(f: impl for<'s> FnOnce(&'s NoGc<'s>) -> R) -> R {
    #[cfg(debug_assertions)]
    struct Guard;
    #[cfg(debug_assertions)]
    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe { js_perry_bytes_scope_leave() };
        }
    }
    #[cfg(debug_assertions)]
    let _guard = {
        check_abi();
        unsafe { js_perry_bytes_scope_enter() };
        Guard
    };
    f(&NoGc(PhantomData))
}
/// Borrow a byte-bearing value until this scope ends; rejects detached values.
pub fn borrow<'s>(value: JsValue, _: &'s NoGc<'s>) -> Option<&'s [u8]> {
    check_abi();
    let mut out = PerryBytes::default();
    if unsafe { js_perry_bytes_borrow(f64::from_bits(value.bits()), &mut out) } != 0 {
        return None;
    }
    Some(unsafe { std::slice::from_raw_parts(out.ptr, out.len) })
}
/// Owner-thread pin whose byte address survives callbacks and detach.
pub struct Pinned {
    bytes: PerryBytes,
    _not_send: PhantomData<std::rc::Rc<()>>,
}
impl Pinned {
    /// Readable native address, valid until this pin is dropped.
    pub fn as_ptr(&self) -> *const u8 {
        self.bytes.ptr
    }
    /// Writable native address; callers must synchronise aliased byte access.
    pub fn as_mut_ptr(&self) -> *mut u8 {
        self.bytes.ptr
    }
    /// Length of the pinned byte range.
    pub fn len(&self) -> usize {
        self.bytes.len
    }
    /// Whether the pinned byte range is empty.
    pub fn is_empty(&self) -> bool {
        self.bytes.len == 0
    }
}
impl Drop for Pinned {
    fn drop(&mut self) {
        unsafe { js_perry_bytes_unpin(self.bytes.pin) };
    }
}
/// Root and pin a live byte-bearing value on its owning thread.
pub fn pin(value: JsValue) -> Option<Pinned> {
    check_abi();
    let mut bytes = PerryBytes::default();
    if unsafe { js_perry_bytes_pin(f64::from_bits(value.bits()), &mut bytes) } != 0 {
        return None;
    }
    Some(Pinned {
        bytes,
        _not_send: PhantomData,
    })
}
#[repr(u32)]
#[derive(Clone, Copy)]
/// JavaScript brand for a newly allocated byte value.
pub enum Brand {
    /// Node Buffer.
    Buffer = 0,
    /// Uint8Array.
    Uint8Array = 1,
    /// ArrayBuffer.
    ArrayBuffer = 2,
    /// DataView.
    DataView = 3,
}
/// Create bytes with a writable pin; initialise the entire range before publishing.
pub fn new_uninit(brand: Brand, len: usize) -> (JsValue, Pinned) {
    check_abi();
    let mut bytes = PerryBytes::default();
    let value = unsafe { js_perry_bytes_new(brand as u32, len, 1, &mut bytes) };
    assert_ne!(bytes.pin, 0, "byte allocation refused");
    (
        JsValue::from_bits(value.to_bits()),
        Pinned {
            bytes,
            _not_send: PhantomData,
        },
    )
}
/// Copy Rust bytes into a runtime-owned byte value.
pub fn from_slice(brand: Brand, input: &[u8]) -> JsValue {
    check_abi();
    JsValue::from_bits(
        unsafe { js_perry_bytes_copy(brand as u32, input.as_ptr(), input.len()) }.to_bits(),
    )
}
unsafe extern "C" fn free_vec(ptr: *mut u8, len: usize, cap: usize) {
    drop(Vec::from_raw_parts(ptr, len, cap));
}
/// Consume Rust bytes using the runtime placement rule.
pub fn from_vec(brand: Brand, mut input: Vec<u8>) -> JsValue {
    check_abi();
    let (ptr, len, cap) = (input.as_mut_ptr(), input.len(), input.capacity());
    std::mem::forget(input);
    JsValue::from_bits(
        unsafe { js_perry_bytes_adopt(brand as u32, ptr, len, cap, free_vec) }.to_bits(),
    )
}
