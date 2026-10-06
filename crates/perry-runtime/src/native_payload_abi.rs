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

unsafe fn checked_family<'a>(family: *const PerryPayloadFamily) -> Option<&'a PerryPayloadFamily> {
    if family.is_null() || (*family).abi != payload_abi_layout() {
        return None;
    }
    let family = &*family;
    (!family.vtable.is_null() && !family.name.is_null()).then_some(family)
}

fn family_type_id(family: &PerryPayloadFamily) -> u64 {
    family.class_id as u64
        | ((family.payload_size as u64 & 0xFF_FFFF) << 32)
        | ((family.payload_align as u64 & 0xFF) << 56)
}

unsafe fn family_cell(
    value: f64,
    family: &PerryPayloadFamily,
) -> Option<*mut crate::native_handle::NativeHandleHeader> {
    let obj = crate::native_payload::any_object(value)?;
    let meta = (*obj).meta;
    if meta.is_null() || !crate::native_payload::is_payload_state_word((*meta).native_state) {
        return None;
    }
    let cell = ((*meta).native_state & crate::value::POINTER_MASK)
        as *mut crate::native_handle::NativeHandleHeader;
    // The attached cell brands subclasses as well as direct instances.
    ((*cell).type_id == family_type_id(family)
        && crate::native_handle::cell_vtable(cell)? as *const _ == family.vtable)
        .then_some(cell)
}

/// Allocate on the binding's canonical constructor prototype, using the same
/// birth shape and traced cell as an in-tree family. On failure ownership of
/// resource stays with the caller.
/// # Safety
/// family and its vtable live forever; resource is a boxed payload of its type.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_alloc(
    family: *const PerryPayloadFamily,
    resource: *mut c_void,
    proto: f64,
    bytes: usize,
) -> f64 {
    let Some(family) = checked_family(family) else {
        return bytes_undefined();
    };
    let Some(proto) = crate::native_payload::any_object(proto) else {
        return bytes_undefined();
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_raw_mut_ptr(proto);
    let obj =
        proto.with_mut_ptr(|proto| crate::native_payload::born_instance(family.class_id, proto, 0));
    let obj = scope.root_raw_mut_ptr(obj);
    let name = std::str::from_utf8(std::slice::from_raw_parts(family.name, family.name_len))
        .unwrap_or("NativePayload");
    crate::native_payload::attach_external_rooted(
        &obj,
        resource,
        family_type_id(family),
        &*family.vtable,
        name,
        family.links_owner != 0,
        bytes,
    );
    obj.with_mut_ptr(|obj: *mut crate::object::ObjectHeader| {
        crate::value::js_nanbox_pointer(obj as i64)
    })
}

/// Attach for a source subclass's super() constructor. Never overwrites an
/// existing cell (in particular, a closed stream cannot reopen).
/// # Safety
/// As js_perry_payload_alloc.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_attach(
    value: f64,
    family: *const PerryPayloadFamily,
    resource: *mut c_void,
    bytes: usize,
) -> i32 {
    let Some(family) = checked_family(family) else {
        return -1;
    };
    let Some(obj) = crate::native_payload::any_object(value) else {
        return -1;
    };
    if !(*obj).meta.is_null()
        && crate::native_payload::is_payload_state_word((*(*obj).meta).native_state)
    {
        return -1;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(obj);
    let name = std::str::from_utf8(std::slice::from_raw_parts(family.name, family.name_len))
        .unwrap_or("NativePayload");
    crate::native_payload::attach_external_rooted(
        &obj,
        resource,
        family_type_id(family),
        &*family.vtable,
        name,
        family.links_owner != 0,
        bytes,
    );
    0
}

/// # Safety
/// family is a static descriptor; the pointer is borrowed only until JS or GC.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_get(
    value: f64,
    family: *const PerryPayloadFamily,
) -> *mut c_void {
    let Some(family) = checked_family(family) else {
        return std::ptr::null_mut();
    };
    let Some(cell) = family_cell(value, family) else {
        return std::ptr::null_mut();
    };
    crate::native_handle::native_handle_rust_payload_ptr(cell, family_type_id(family))
}

/// # Safety
/// family is a static descriptor. Close preserves the cell and its vtable.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_close(
    value: f64,
    family: *const PerryPayloadFamily,
) -> i32 {
    let Some(family) = checked_family(family) else {
        return -1;
    };
    let Some(cell) = family_cell(value, family) else {
        return -1;
    };
    if crate::native_handle::native_handle_rust_payload_ptr(cell, family_type_id(family)).is_null()
    {
        return 0;
    }
    if (*cell).busy != 0 {
        (*cell).flags |= crate::native_payload::CLOSING;
    } else {
        crate::native_handle::native_handle_release_rust_payload(cell);
    }
    0
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
    #[used(compiler)]
    static PAYLOAD_ALLOC: unsafe extern "C" fn(
        *const PerryPayloadFamily,
        *mut c_void,
        f64,
        usize,
    ) -> f64 = js_perry_payload_alloc;
    #[used(compiler)]
    static PAYLOAD_ATTACH: unsafe extern "C" fn(
        f64,
        *const PerryPayloadFamily,
        *mut c_void,
        usize,
    ) -> i32 = js_perry_payload_attach;
    #[used(compiler)]
    static PAYLOAD_GET: unsafe extern "C" fn(f64, *const PerryPayloadFamily) -> *mut c_void =
        js_perry_payload_get;
    #[used(compiler)]
    static PAYLOAD_CLOSE: unsafe extern "C" fn(f64, *const PerryPayloadFamily) -> i32 =
        js_perry_payload_close;
    #[used(compiler)]
    static PAYLOAD_PROTOTYPE: unsafe extern "C" fn(
        *const PerryPayloadFamily,
        *const u8,
        usize,
    ) -> f64 = js_perry_payload_prototype;
    #[used(compiler)]
    static PAYLOAD_PROTO_METHOD: unsafe extern "C" fn(
        *mut c_void,
        *const u8,
        usize,
        *const crate::closure::JsFunctionInfo,
        u32,
    ) = js_perry_payload_proto_method;
    #[used(compiler)]
    static PAYLOAD_OWN: unsafe extern "C" fn(f64, *const u8, usize, f64) = js_perry_payload_own;
    #[used(compiler)]
    static PAYLOAD_GET_ATTACHED: unsafe extern "C" fn(
        f64,
        *const crate::native_payload::PayloadVTable,
    ) -> *mut c_void = js_perry_payload_get_attached;
    #[used(compiler)]
    static PAYLOAD_CLOSE_ATTACHED: unsafe extern "C" fn(
        f64,
        *const crate::native_payload::PayloadVTable,
    ) -> i32 = js_perry_payload_close_attached;
    #[used(compiler)]
    static PAYLOAD_EXTERNAL_BYTES: unsafe extern "C" fn(
        f64,
        *const crate::native_payload::PayloadVTable,
        usize,
    ) = js_perry_payload_external_bytes;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    per_test_global! { static DROPS: AtomicUsize = AtomicUsize::new(0); }
    unsafe extern "C" fn drop_bytes(resource: *mut c_void, _: *mut c_void) {
        drop(Box::from_raw(resource as *mut Vec<u8>));
        DROPS.fetch_add(1, Ordering::SeqCst);
    }
    static VTABLE: crate::native_payload::PayloadVTable = crate::native_payload::PayloadVTable {
        drop: drop_bytes,
        stream: None,
    };
    #[test]
    fn external_family_uses_the_shared_cell_and_close_never_reopens() {
        DROPS.store(0, Ordering::SeqCst);
        let family = PerryPayloadFamily {
            abi: payload_abi_layout(),
            class_id: crate::native_class_ids::CRYPTO_HASH,
            links_owner: 0,
            constructor_length: 1,
            _reserved: 0,
            name: b"Bytes".as_ptr(),
            name_len: 5,
            install_prototype: None,
            vtable: &VTABLE,
            payload_size: std::mem::size_of::<Vec<u8>>(),
            payload_align: std::mem::align_of::<Vec<u8>>(),
        };
        let scope = crate::gc::RuntimeHandleScope::new();
        let proto = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        let data = Box::into_raw(Box::new(vec![19u8; 8192]));
        let obj = scope.root_nanbox_f64(unsafe {
            js_perry_payload_alloc(&family, data.cast(), proto.get_nanbox_f64(), 8192)
        });
        unsafe {
            let cell = family_cell(obj.get_nanbox_f64(), &family).unwrap();
            assert_eq!((*cell).external_bytes, 8192);
            assert_eq!(
                (*cell).owner,
                0,
                "links_owner:false must leave no owner edge"
            );
            assert_eq!(
                js_perry_payload_get(obj.get_nanbox_f64(), &family),
                data.cast()
            );
            assert_eq!(js_perry_payload_close(obj.get_nanbox_f64(), &family), 0);
            assert_eq!((*cell).external_bytes, 0, "close releases bytes before GC");
            assert_eq!(DROPS.load(Ordering::SeqCst), 1);
            assert!(js_perry_payload_get(obj.get_nanbox_f64(), &family).is_null());
            assert_eq!(js_perry_payload_close(obj.get_nanbox_f64(), &family), 0);
            assert_eq!(
                js_perry_payload_attach(obj.get_nanbox_f64(), &family, std::ptr::null_mut(), 0),
                -1
            );
        }
        assert_eq!(DROPS.load(Ordering::SeqCst), 1);
    }
}

/// Canonical constructor prototype, adopting it into the existing per-realm
/// payload prototype slots. The installer runs once, preserving user changes.
/// # Safety
/// descriptor and byte strings are valid; descriptor and installer are static.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_prototype(
    family: *const PerryPayloadFamily,
    module: *const u8,
    module_len: usize,
) -> f64 {
    let Some(family) = checked_family(family) else {
        return bytes_undefined();
    };
    let Ok(module) = std::str::from_utf8(std::slice::from_raw_parts(module, module_len)) else {
        return bytes_undefined();
    };
    let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(family.name, family.name_len))
    else {
        return bytes_undefined();
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let ctor = scope.root_nanbox_f64(crate::object::bound_native_callable_export_value(
        module, name,
    ));
    let proto = scope.root_nanbox_f64(crate::object::js_function_prototype_value_for_read(
        ctor.get_nanbox_f64(),
    ));
    let Some(ptr) = crate::native_payload::any_object(proto.get_nanbox_f64()) else {
        return bytes_undefined();
    };
    let ptr = crate::native_payload::adopt_prototype_with(family.class_id, ptr, |proto| {
        if let Some(install) = family.install_prototype {
            install(proto.cast());
        }
    });
    crate::value::js_nanbox_pointer(ptr as i64)
}

/// # Safety
/// byte name is valid; info is a static, ABI-compatible function descriptor.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_proto_method(
    proto: *mut c_void,
    name: *const u8,
    name_len: usize,
    info: *const crate::closure::JsFunctionInfo,
    arity: u32,
) {
    let name = std::str::from_utf8(std::slice::from_raw_parts(name, name_len)).unwrap();
    crate::object::install_proto_method(proto.cast(), name, info, arity);
}

/// Write a normal enumerable own field in constructor order.
/// # Safety
/// key is a valid UTF-8 span; owner is an ordinary object.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_own(
    owner: f64,
    key: *const u8,
    key_len: usize,
    value: f64,
) {
    let Some(obj) = crate::native_payload::any_object(owner) else {
        return;
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(obj);
    crate::native_payload::set_own(
        &scope,
        &obj,
        std::slice::from_raw_parts(key, key_len),
        value,
    );
}

unsafe fn attached_cell(
    value: f64,
    vtable: *const crate::native_payload::PayloadVTable,
) -> Option<*mut crate::native_handle::NativeHandleHeader> {
    let obj = crate::native_payload::any_object(value)?;
    let meta = (*obj).meta;
    if meta.is_null() || !crate::native_payload::is_payload_state_word((*meta).native_state) {
        return None;
    }
    let cell = ((*meta).native_state & crate::value::POINTER_MASK)
        as *mut crate::native_handle::NativeHandleHeader;
    (crate::native_handle::cell_vtable(cell)? as *const _ == vtable).then_some(cell)
}
/// # Safety
/// vtable describes the payload's Rust type and lives forever.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_get_attached(
    value: f64,
    vtable: *const crate::native_payload::PayloadVTable,
) -> *mut c_void {
    let Some(cell) = attached_cell(value, vtable) else {
        return std::ptr::null_mut();
    };
    crate::native_handle::native_handle_rust_payload_ptr(cell, (*cell).type_id)
}
/// # Safety
/// As js_perry_payload_get_attached. Close never changes the vtable or reopens.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_close_attached(
    value: f64,
    vtable: *const crate::native_payload::PayloadVTable,
) -> i32 {
    let Some(cell) = attached_cell(value, vtable) else {
        return -1;
    };
    if crate::native_handle::native_handle_rust_payload_ptr(cell, (*cell).type_id).is_null() {
        return 0;
    }
    if (*cell).busy != 0 {
        (*cell).flags |= crate::native_payload::CLOSING;
    } else {
        crate::native_handle::native_handle_release_rust_payload(cell);
    }
    0
}

/// # Safety
/// vtable is the static payload type; owner is rooted by the caller.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_external_bytes(
    value: f64,
    vtable: *const crate::native_payload::PayloadVTable,
    bytes: usize,
) {
    if let Some(cell) = attached_cell(value, vtable) {
        crate::native_handle::native_handle_set_external_bytes(cell, bytes);
    }
}
