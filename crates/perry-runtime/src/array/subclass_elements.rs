//! Elements backing store for `class X extends Array` instances.
//!
//! An Array-subclass instance is an ordinary `GC_TYPE_OBJECT` (`super::subclass`);
//! today its indexed elements and `length` are shape-carried properties, so every
//! `push`/`pop`/`obj[i] = v` is a property-shape transition. Under
//! [`array_subclass_elements_enabled`] the instance instead owns a real
//! `GC_TYPE_ARRAY` in `ObjectMeta.elements` (a traced child edge exactly like
//! `spill`) holding its indexed elements and `length`, and the property entry
//! points route canonical array-index keys and `length` to it.
//!
//! This module owns the edge: the gate, the accessor, installation at
//! construction, and the barriered head write-back after a re-allocating append.
use crate::array::ArrayHeader;
use crate::object::ObjectHeader;

/// `PERRY_ARRAY_SUBCLASS_ELEMENTS=1|on|true` — off while the property entry
/// points are being routed; the default flips once the semantics suite is green.
#[inline]
pub(crate) fn array_subclass_elements_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        matches!(
            std::env::var("PERRY_ARRAY_SUBCLASS_ELEMENTS").as_deref(),
            Ok("1") | Ok("on") | Ok("true")
        )
    })
}

/// The elements store of a live `GC_TYPE_OBJECT`, or null when it has none
/// (no meta record, or not an elements-backed Array subclass instance).
///
/// # Safety
/// `obj` must be a live `GC_TYPE_OBJECT` user pointer.
#[inline]
pub(crate) unsafe fn elements_of(obj: *const ObjectHeader) -> *mut ArrayHeader {
    let meta = (*obj).meta;
    if meta.is_null() {
        return std::ptr::null_mut();
    }
    (*meta).elements as *mut ArrayHeader
}

/// Store `elements` as the instance's backing store (a barriered meta-record
/// slot store; `elements` may be null to detach, e.g. on deopt).
///
/// # Safety
/// `obj` must be a live `GC_TYPE_OBJECT` with a meta record.
#[inline]
pub(crate) unsafe fn set_elements_head(obj: *mut ObjectHeader, elements: *mut ArrayHeader) {
    let meta = (*obj).meta;
    debug_assert!(!meta.is_null());
    // GC_STORE_AUDIT(BARRIERED): meta-record child edge, stored exactly as
    // `reserve_object_spill` stores `spill`.
    (*meta).elements = elements as u64;
    crate::gc::runtime_write_barrier_slot(
        meta as usize,
        &(*meta).elements as *const _ as usize,
        elements as u64,
    );
}

/// Install a fresh elements store of `length` holes on `obj` (the `super(n)`
/// shape: `length = n`, every index absent). Idempotent: an instance that
/// already has a store keeps it.
///
/// # Safety
/// `obj` must be a live `GC_TYPE_OBJECT` user pointer.
pub(crate) unsafe fn install_elements(obj: *mut ObjectHeader, length: u32) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let (_, obj) =
        obj_handle.across_mut::<ObjectHeader, _>(|| crate::object::object_meta_ensure(obj));
    if !elements_of(obj).is_null() {
        return;
    }
    let (elements, obj) = obj_handle
        .across_mut::<ObjectHeader, _>(|| crate::array::js_array_alloc_with_length_exact(length));
    if elements_of(obj).is_null() {
        set_elements_head(obj, elements);
    }
}
