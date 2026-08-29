//! Reserved raw-field floors for built-in iterator objects (#9019).
//!
//! The built-in iterator families (array / map / set / string / buffer /
//! regexp-string iterators, iterator helpers) are ordinary `GC_TYPE_OBJECT`
//! allocations whose internal state — backing collection, cursor, kind,
//! cached result — lives in RAW numbered fields written with
//! `js_object_set_field`, while their keys array starts EMPTY. The by-name
//! append path derives a new key's field index from the keys array, so the
//! first user property (`it.next = fn`, `it.foo = 1`) landed at field index
//! 0 and overwrote the backing-collection pointer. The next builtin
//! `.next()` then read the stored value as a `SetHeader`/`MapHeader`
//! pointer: a NaN-boxed number there reads as a null-ish backing (the
//! iterator silently reports `done: true`), a closure there is dereferenced
//! as a collection header and SIGSEGVs.
//!
//! The fix keeps the keys-position ↔ field-index correspondence every
//! by-name path relies on: before the FIRST by-name append to such a
//! receiver, seed its keys array with `floor` leading tombstones
//! (`TAG_HOLE`, the #9038 hole-delete marker every lookup / enumeration /
//! delete path already skips). User keys then append from `floor` upward —
//! past every raw internal field — and land in the ordinary inline/overflow
//! storage. Unpatched iterators never pay: the seed runs only when user
//! code actually adds a named property.

use super::ObjectHeader;
use crate::array::ArrayHeader;

/// One past the highest raw numbered field the family's dispatch touches.
/// `0` for every class id without a reserved raw-field layout. Keep each
/// entry in lock-step with the family's allocator/dispatcher:
///
///   * array:  `array/iter_object.rs` (fields 0..4: backing, cursor, kind,
///     snapshot len, epoch)
///   * map/set: `collection_iter_object.rs` (fields 0..5: backing, cursor,
///     kind, size-at-last-next, last key, cached fused result)
///   * string: `string/iter_object.rs` (fields 0..1)
///   * buffer: `buffer/iter.rs` (fields 0..2)
///   * regexp-string: `regex/match_all.rs` (fields 0..1)
///   * iterator helpers: `iterator_helpers.rs` (fields 0..3)
pub(crate) fn reserved_slot_floor_for_class_id(class_id: u32) -> u32 {
    match class_id {
        crate::array::ARRAY_ITERATOR_CLASS_ID => 5,
        crate::collection_iter_object::MAP_ITERATOR_CLASS_ID
        | crate::collection_iter_object::SET_ITERATOR_CLASS_ID => 6,
        crate::string::STRING_ITERATOR_CLASS_ID => 2,
        crate::buffer::BUFFER_ITERATOR_CLASS_ID => 3,
        crate::regex::REGEXP_STRING_ITERATOR_CLASS_ID => 2,
        crate::iterator_helpers::ITERATOR_HELPER_CLASS_ID => 4,
        _ => 0,
    }
}

/// Install the reserved-floor keys array on a keys-less receiver whose class
/// id reserves raw field slots: `floor` leading `TAG_HOLE` slots, published
/// as a shape whose `hole_count` matches the physical holes, preserving the
/// birth descriptor's live inline-slot bound / kind / generation. Returns
/// `true` when the seed was installed — the keys allocation can trigger a
/// collection that MOVES the receiver, so the caller must re-read every raw
/// pointer (including its `keys` edge) through its handles afterwards.
///
/// # Safety
/// `obj` must be a live `GC_TYPE_OBJECT` allocation.
pub(crate) unsafe fn ensure_reserved_floor_keys(obj: *mut ObjectHeader) -> bool {
    let floor = reserved_slot_floor_for_class_id((*obj).class_id);
    if floor == 0 || !super::object_keys_array(obj).is_null() {
        return false;
    }
    // NaN-boxed handles rather than `root_raw_*_ptr`, so every reload is a
    // `get_nanbox_f64` at the point of use and this module stays out of
    // `scripts/raw_handle_debt.py`'s ledger (same idiom as
    // `array/iterator.rs::js_iterator_to_array`).
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(obj as i64));
    let keys = crate::array::js_array_alloc(floor);
    if keys.is_null() {
        return false;
    }
    let keys_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(keys as i64));
    let keys = crate::value::js_nanbox_get_pointer(keys_h.get_nanbox_f64()) as *mut ArrayHeader;
    for i in 0..floor as usize {
        crate::array::store_array_slot(keys, i, crate::value::TAG_HOLE);
    }
    (*keys).length = floor;
    crate::array::rebuild_array_layout_exact(keys);
    // Reload both through their handles before publishing: nothing between
    // the allocation and here allocates, but the publish path must never
    // hold a pre-collection address.
    let obj = crate::value::js_nanbox_get_pointer(obj_h.get_nanbox_f64()) as *mut ObjectHeader;
    let keys = crate::value::js_nanbox_get_pointer(keys_h.get_nanbox_f64()) as *mut ArrayHeader;
    // The keys edge is changing: retire any typed layout trained on the old
    // (keys-less) representation before the successor is published, exactly
    // like `set_object_keys_array_with_live`.
    super::mark_object_dynamic_shape_unknown(obj);
    stamp_reserved_floor_shape(obj, keys, floor) != 0
}

/// Publish + stamp the reserved-floor descriptor: `floor` keys, all of them
/// holes, at the receiver's current live inline-slot bound. Mirrors
/// `shapes::stamp_object_shape`'s lineage handling but carries an explicit
/// `hole_count` so the tombstone bookkeeping (delete thresholds, the
/// floor-aware squeeze in `delete_rest.rs`) sees the physical holes.
unsafe fn stamp_reserved_floor_shape(
    obj: *mut ObjectHeader,
    keys: *const ArrayHeader,
    floor: u32,
) -> u32 {
    use super::shapes;
    if !shapes::shape_word_is_writable(obj) {
        return 0;
    }
    let lineage = shapes::object_shape_descriptor(obj);
    let live = lineage
        .as_ref()
        .map(|d| d.live_inline_slot_count)
        .unwrap_or(0);
    let generation = lineage.as_ref().map(|d| d.semantic_generation).unwrap_or(0);
    let kind = lineage
        .as_ref()
        .map(|d| d.object_kind)
        .unwrap_or(shapes::ShapeObjectKind::Ordinary);
    crate::array::clear_array_subclass_named_prefix_token(obj);
    let id = shapes::publish_shape_result(shapes::shape_descriptor_ensure_with_holes(
        keys, floor, live, generation, kind, floor,
    ));
    (*obj).parent_class_id = id;
    shapes::debug_assert_object_shape_parity_for_keys(obj, keys as *mut ArrayHeader);
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{
        js_object_get_field, js_object_get_own_field_or_undef, js_object_set_field_by_name,
    };
    use crate::value::{js_nanbox_get_pointer, js_nanbox_pointer, JSValue};

    unsafe fn set_iter_with_10_20_30() -> *mut ObjectHeader {
        let set = crate::set::js_set_alloc(4);
        for v in [10.0f64, 20.0, 30.0] {
            crate::set::js_set_add(set, v);
        }
        js_nanbox_get_pointer(js_nanbox_pointer(
            crate::collection_iter_object::js_set_values_iter_obj(set),
        )) as *mut ObjectHeader
    }

    unsafe fn key(name: &str) -> *const crate::StringHeader {
        crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
    }

    unsafe fn result_value(res: f64) -> f64 {
        f64::from_bits(
            js_object_get_field(js_nanbox_get_pointer(res) as *mut ObjectHeader, 0).bits(),
        )
    }

    unsafe fn result_done(res: f64) -> bool {
        crate::value::js_is_truthy(f64::from_bits(
            js_object_get_field(js_nanbox_get_pointer(res) as *mut ObjectHeader, 1).bits(),
        )) != 0
    }

    /// #9019 regression, storage half: a by-name write on a Set iterator
    /// must land PAST the raw internal fields. Pre-fix, `it.foo = 123`
    /// stored 123 into field 0 — the backing-Set pointer — so the next
    /// `.next()` reported `done: true` on a set with three live elements
    /// (and a closure stored there was dereferenced as a `SetHeader` and
    /// crashed). The fixture asserts the DISCRIMINATING quantity: field 0
    /// still holds the original backing value after the write.
    #[test]
    fn by_name_write_does_not_alias_the_backing_collection_field() {
        unsafe {
            let iter = set_iter_with_10_20_30();
            let backing_before = js_object_get_field(iter, 0).bits();
            assert!(
                JSValue::from_bits(backing_before).is_pointer(),
                "fixture must start with a pointer backing in field 0, or \
                 every verdict below is vacuous"
            );

            js_object_set_field_by_name(iter, key("foo"), 123.0);

            assert_eq!(
                js_object_get_field(iter, 0).bits(),
                backing_before,
                "the named write must not overwrite the backing-Set field"
            );
            let own = js_object_get_own_field_or_undef(
                js_nanbox_pointer(iter as i64),
                b"foo".as_ptr(),
                3,
            );
            assert_eq!(own, 123.0, "the named property must read back by name");

            // And the iterator still walks all three elements.
            let r1 = crate::collection_iter_object::dispatch_set_iterator_method(iter, "next");
            assert_eq!(result_value(r1), 10.0);
            assert!(!result_done(r1));
        }
    }

    /// #9019, dispatch half: an OWN `next` assigned onto the iterator must
    /// win over the builtin advance on the class-id dispatch path (the same
    /// path `for…of`'s fused `js_for_of_next` takes).
    #[test]
    fn own_next_shadows_the_builtin_advance() {
        extern "C" fn patched_next(_c: *const crate::closure::ClosureHeader, _arg: f64) -> f64 {
            unsafe { crate::iter_result::make_iter_result(JSValue::number(777.0), false) }
        }
        unsafe {
            let iter = set_iter_with_10_20_30();
            let closure = crate::closure::js_closure_alloc(patched_next as *const u8, 0);
            assert!(!closure.is_null());
            crate::closure::js_register_closure_arity(patched_next as *const u8, 0);
            js_object_set_field_by_name(
                iter,
                key("next"),
                crate::value::js_nanbox_pointer(closure as i64),
            );

            let r = crate::collection_iter_object::dispatch_set_iterator_method(iter, "next");
            assert_eq!(
                result_value(r),
                777.0,
                "an own patched next must drive the dispatch"
            );
            assert!(!result_done(r));

            // The backing set is untouched: removing the patch is not
            // required for this fixture, but the raw fields must be intact.
            assert!(
                JSValue::from_bits(js_object_get_field(iter, 0).bits()).is_pointer(),
                "backing field survived the patch write"
            );
        }
    }

    /// The seeded floor holds across every reserved family, and ordinary
    /// receivers are untouched (`floor == 0`).
    #[test]
    fn floors_cover_every_reserved_family_and_nothing_else() {
        assert_eq!(
            reserved_slot_floor_for_class_id(crate::array::ARRAY_ITERATOR_CLASS_ID),
            5
        );
        assert_eq!(
            reserved_slot_floor_for_class_id(crate::collection_iter_object::MAP_ITERATOR_CLASS_ID),
            6
        );
        assert_eq!(
            reserved_slot_floor_for_class_id(crate::collection_iter_object::SET_ITERATOR_CLASS_ID),
            6
        );
        assert_eq!(
            reserved_slot_floor_for_class_id(crate::string::STRING_ITERATOR_CLASS_ID),
            2
        );
        assert_eq!(
            reserved_slot_floor_for_class_id(crate::buffer::BUFFER_ITERATOR_CLASS_ID),
            3
        );
        assert_eq!(
            reserved_slot_floor_for_class_id(crate::regex::REGEXP_STRING_ITERATOR_CLASS_ID),
            2
        );
        assert_eq!(
            reserved_slot_floor_for_class_id(crate::iterator_helpers::ITERATOR_HELPER_CLASS_ID),
            4
        );
        assert_eq!(reserved_slot_floor_for_class_id(0), 0);
        assert_eq!(reserved_slot_floor_for_class_id(42), 0);
    }

    /// Deleting the patch tombstones it without disturbing the reserved
    /// prefix, and the builtin advance resumes.
    #[test]
    fn delete_of_a_user_key_keeps_the_reserved_prefix() {
        unsafe {
            let iter = set_iter_with_10_20_30();
            let backing_before = js_object_get_field(iter, 0).bits();
            js_object_set_field_by_name(iter, key("foo"), 123.0);
            let deleted = crate::object::js_object_delete_field(iter, key("foo"));
            assert_eq!(deleted, 1, "delete must report success");
            assert_eq!(
                js_object_get_field(iter, 0).bits(),
                backing_before,
                "delete must not disturb the backing field"
            );
            let r = crate::collection_iter_object::dispatch_set_iterator_method(iter, "next");
            assert_eq!(result_value(r), 10.0);
        }
    }
}
