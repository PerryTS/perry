//! Class-less ORDINARY births (#8098, charter step 3).
//!
//! `OBJ_FLAG_PLAIN_ORDINARY` is an input of the receiver's store kind
//! (`shapes::store_kind`): a receiver born with it carries an `Ordinary`
//! shape, one without it an `OrdinaryUnmarked` shape. A birth site that has
//! established its receiver is ordinary marks it BEFORE the first stamp, so
//! the birth mint is `Ordinary` and no twin is ever minted.

use super::*;
use crate::array::ArrayHeader;

/// #8098: mark `obj` as an ORDINARY plain object — class-less, but with no
/// per-object `[[Set]]` semantics of its own, so the object-write fast paths
/// may treat it exactly like a class instance.
///
/// The mark is deliberately OPT-IN and set at BIRTH. `class_id == 0` is not a
/// sufficient condition: a `URL` instance, `Object.prototype`, a module
/// namespace, and a native-module receiver are all class-less, and the write
/// guards used to exclude the whole class-less population wholesale rather than
/// reason about them (`proxy/put_value.rs`, and the same three exclusions in
/// `field_set_by_name/fast_paths.rs::try_existing_own_data_overwrite`). Only a
/// birth site that has established its receiver is ordinary calls this; every
/// other class-less receiver keeps taking the full `[[Set]]` walk.
///
/// The bit lives in `GcHeader::_reserved`, which survives evacuation
/// (`gc/copying.rs` and `gc/oldgen.rs` carry the word across), is preserved by
/// the survival-age (`0x0038`) and layout-state (`0xC000`) updates, and is
/// already loaded by the generated write PIC for its blocking-flag test.
///
/// Charter step 3: the mark is an input of the receiver's store kind
/// (`shapes::store_kind`), so setting it moves an already-stamped receiver to
/// its `Ordinary` twin. A birth site that marks before its first stamp
/// (`shapes::store_kind::premark_plain_ordinary`) mints `Ordinary` directly.
#[inline]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) unsafe fn mark_object_plain_ordinary(obj: *mut ObjectHeader) {
    crate::object::shapes::store_kind::mark_plain_ordinary(obj);
}

/// A class-less `JSON.parse` record with a known keys view, born marked
/// plain-ordinary (`mark_object_plain_ordinary`) before its first stamp.
pub(crate) fn alloc_plain_record_with_keys(
    field_count: u32,
    keys: crate::object::ObjectKeys,
) -> *mut ObjectHeader {
    alloc_class_instance_with_keys_impl(0, 0, field_count, keys, true)
}

/// A class-less `JSON.parse` record from a cached keys global and ShapeId,
/// born marked plain-ordinary before its first stamp (charter step 3).
pub(crate) fn alloc_plain_record_inline_keys_stamped(
    field_count: u32,
    keys_array: *mut ArrayHeader,
    shape_id: u32,
) -> *mut ObjectHeader {
    alloc_class_inline_keys_stamped_impl(0, 0, field_count, keys_array, shape_id, true)
}

/// The runtime class-instance allocation, optionally born marked.
pub(super) fn alloc_class_instance_with_keys_impl(
    class_id: u32,
    parent_class_id: u32,
    field_count: u32,
    keys: crate::object::ObjectKeys,
    premark_plain: bool,
) -> *mut ObjectHeader {
    let (ptr, birth_slots, _, keys) = super::alloc::object_alloc_class_inline_keys_impl(
        class_id,
        parent_class_id,
        field_count,
        keys,
        0,
        premark_plain,
    );
    unsafe {
        let id = crate::object::shapes::shape_id_for_class_keys_ensure(
            keys.arr() as *const ArrayHeader,
            keys.count(),
            class_id,
        );
        crate::object::shapes::birth_stamp_object_shape(ptr, id, birth_slots);
    }
    ptr
}

/// The compiled-class allocation from a module-init ShapeId, optionally born
/// marked.
pub(super) fn alloc_class_inline_keys_stamped_impl(
    class_id: u32,
    parent_class_id: u32,
    field_count: u32,
    keys_array: *mut ArrayHeader,
    shape_id: u32,
    premark_plain: bool,
) -> *mut ObjectHeader {
    let keys = super::alloc::preinstalled_class_keys(keys_array, shape_id);
    let (ptr, birth_slots, used_preinstalled_shape, _) =
        super::alloc::object_alloc_class_inline_keys_impl(
            class_id,
            parent_class_id,
            field_count,
            keys,
            shape_id,
            premark_plain,
        );
    if !used_preinstalled_shape {
        unsafe {
            crate::object::shapes::birth_stamp_object_shape(ptr, shape_id, birth_slots);
        }
    }
    ptr
}
