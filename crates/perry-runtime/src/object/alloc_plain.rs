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
    let rep = super::field_rep::REP_ANY;
    alloc_class_inline_keys_stamped_impl(0, 0, field_count, keys_array, shape_id, rep, true)
}

/// The shape-cache slot of `class_id`'s keys built with `field_count` keys
/// (`js_build_class_keys_array`, `js_object_alloc_class_with_keys`).
pub(super) fn class_keys_cache_slot(class_id: u32, field_count: u32) -> u32 {
    class_id
        .wrapping_mul(10007)
        .wrapping_add(field_count.wrapping_mul(100003))
        .wrapping_add(1000000)
}

/// The birth rep of `class_id`'s instances, read off the shape its module
/// init minted beside its canonical keys (`js_build_class_keys_array` mints
/// that shape with the class's birth rep): the shape is the record, so no
/// table carries the rep. `REP_ANY` for a class-less birth, or when the slot
/// names other keys (or nothing).
pub(super) fn class_keys_birth_rep(
    class_id: u32,
    field_count: u32,
    keys: crate::object::ObjectKeys,
) -> u64 {
    if class_id == 0 {
        return super::field_rep::REP_ANY;
    }
    let (cached, id) = super::shape_cache_get_with_id(class_keys_cache_slot(class_id, field_count));
    if id == 0 || cached.arr() != keys.arr() || cached.count() != keys.count() {
        return super::field_rep::REP_ANY;
    }
    shape_rep_of(id)
}

/// The birth rep an id names (`F64` for a lane its lineage has since
/// deprecated: a birth still carries it, `birth_fill_f64_lanes`).
pub(super) fn shape_rep_of(shape_id: u32) -> u64 {
    crate::object::shapes::shape_record_by_id(shape_id).map_or(super::field_rep::REP_ANY, |r| {
        super::field_rep::identity(r.rep())
    })
}

/// The runtime class-instance allocation, optionally born marked.
pub(super) fn alloc_class_instance_with_keys_impl(
    class_id: u32,
    parent_class_id: u32,
    field_count: u32,
    keys: crate::object::ObjectKeys,
    premark_plain: bool,
) -> *mut ObjectHeader {
    // Read before the allocation: `keys` is current only until then.
    let rep = class_keys_birth_rep(class_id, field_count, keys);
    let (ptr, birth_slots, _, keys) = super::alloc::object_alloc_class_inline_keys_impl(
        class_id,
        parent_class_id,
        field_count,
        keys,
        0,
        premark_plain,
    );
    unsafe {
        // The class's birth shape with its birth rep: the same id its
        // compiled `new` sites stamp when the live bound agrees.
        let id = crate::object::shapes::publish_shape_result(
            crate::object::shapes::class_birth_shape_ensure(
                keys.arr() as *const ArrayHeader,
                keys.count(),
                birth_slots,
                class_id,
                rep,
                None,
            ),
        );
        crate::object::shapes::birth_stamp_object_shape(ptr, id, birth_slots, rep);
        if rep != super::field_rep::REP_ANY {
            crate::object::field_rep_store::birth_fill_f64_lanes(ptr);
        }
    }
    ptr
}

/// The compiled-class allocation from a module-init ShapeId and the birth rep
/// codegen gave that id, optionally born marked.
pub(super) fn alloc_class_inline_keys_stamped_impl(
    class_id: u32,
    parent_class_id: u32,
    field_count: u32,
    keys_array: *mut ArrayHeader,
    shape_id: u32,
    rep: u64,
    premark_plain: bool,
) -> *mut ObjectHeader {
    let keys = preinstalled_class_keys(keys_array, shape_id);
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
            crate::object::shapes::birth_stamp_object_shape(ptr, shape_id, birth_slots, rep);
        }
    }
    // T1: a class birth id's `F64` lanes start as +0.0 (the shape decides,
    // whichever id the object ended up carrying).
    unsafe { crate::object::field_rep_store::birth_fill_f64_lanes(ptr) };
    ptr
}

/// A class keys global's keys, with the count its module-init ShapeId names.
/// A worker installs that id with its own canonical backing, so a global
/// owned by the spawning arena must resolve through the worker's descriptor.
/// A different LOCAL array still takes the exact-array fallback: its facts
/// can diverge from the id beside it, so the birth stamp must validate them.
#[inline]
fn preinstalled_class_keys(
    keys_array: *mut ArrayHeader,
    shape_id: u32,
) -> crate::object::ObjectKeys {
    let descriptor = crate::object::shapes::shape_descriptor_by_id(shape_id);
    if let Some(descriptor) = descriptor {
        if descriptor.keys != keys_array as u64
            && !keys_array.is_null()
            // The global can belong to the spawning arena. Check ownership
            // before reading its header; the ShapeId already names this
            // agent's canonical keys, copied by install_worker_shape_seed.
            && unsafe {
                crate::value::addr_class::try_read_tracked_gc_header(keys_array as usize)
            }
            .is_none()
        {
            return descriptor.keys_view();
        }
    }
    // SAFETY: a local module keys global is a live keys array (or null).
    let owned = unsafe { crate::object::ObjectKeys::owned(keys_array) };
    match descriptor {
        Some(descriptor)
            if descriptor.keys == keys_array as u64
                && descriptor.logical_key_count <= owned.count() =>
        {
            descriptor.keys_view()
        }
        _ => owned,
    }
}
