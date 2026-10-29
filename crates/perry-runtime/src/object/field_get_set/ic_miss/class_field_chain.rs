//! DefineField's fixed-order transitions belong to the receiver shape.
//!
//! A scalar weak successor is enough: its immutable last key names the field,
//! its logical count names the slot, and its rep names the store contract.
//! No atom lookup, global transition entry or heap-pointer root is needed.
//! Cold misses use the shared definition and teach only an exact append.

use crate::codegen_abi::LayoutWord;
use crate::object::{shapes, ObjectHeader};

unsafe fn receiver_record(receiver: f64) -> Option<(*mut ObjectHeader, shapes::ShapeRecordRef)> {
    let bits = receiver.to_bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    let gc = crate::value::addr_class::try_read_gc_header(addr)?;
    const BLOCKING: u16 = crate::gc::OBJ_FLAG_FROZEN
        | crate::gc::OBJ_FLAG_SEALED
        | crate::gc::OBJ_FLAG_NO_EXTEND
        | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
        | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO
        | crate::gc::OBJ_FLAG_STABLE_TOMBSTONES;
    if crate::gc_type_is_not_object!(gc.obj_type)
        || gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || gc._reserved & BLOCKING != 0
    {
        return None;
    }
    let obj = addr as *mut ObjectHeader;
    let record = shapes::shape_record_of_receiver((*obj).shape_word())?;
    if record.object_kind() != shapes::ShapeObjectKind::Ordinary || record.summary() != 0 {
        return None;
    }
    Some((obj, record))
}

/// A hit is noncollecting; false leaves all operands untouched.
pub(super) unsafe fn try_define(receiver: f64, key: f64, value: f64) -> bool {
    let Some((obj, from)) = receiver_record(receiver) else {
        return false;
    };
    let target = from.field_define_target();
    let Some(to) = shapes::shape_record_of_receiver((target).0) else {
        return false;
    };
    let slot = from.logical_key_count();
    if to.logical_key_count() != slot + 1
        || to.object_kind() != from.object_kind()
        || to.proto_id() != from.proto_id()
        || to.summary() != 0
    {
        return false;
    }
    let (words, len) = crate::object::keys_array_dense_slots_resolved(
        to.keys() as usize as *const crate::ArrayHeader
    );
    if slot as usize >= len {
        return false;
    }
    let stored = (*words.add(slot as usize)).to_bits();
    if stored != key.to_bits() {
        let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
        let Some(bytes) = crate::string::js_string_key_bytes(
            crate::JSValue::from_bits(key.to_bits()),
            &mut scratch,
        ) else {
            return false;
        };
        if !crate::string::js_string_key_matches_bytes(crate::JSValue::from_bits(stored), bytes) {
            return false;
        }
    }
    // Refuse growth before changing membership. A reservation/floor is
    // physical storage; the target still decides which slots are visible.
    if to.live_inline_slot_count() > crate::object::object_inline_capacity(obj)
        || (slot >= to.live_inline_slot_count()
            && !crate::object::spill_store_would_be_in_capacity(obj as usize, slot as usize))
        || crate::object::field_rep::has_deprecated(to.rep())
        || crate::object::field_rep::slot_rep(to.rep(), slot)
            == crate::object::field_rep::REP_SPECIAL
        || (crate::object::field_rep::slot_rep(to.rep(), slot) == crate::object::field_rep::REP_F64
            && crate::object::field_rep::f64_slot_bits(value.to_bits()).is_none())
    {
        return false;
    }
    // A1_UNPROVEN: record-first: ic_miss entry (kind-A/B question for S0f).
    // DefineField of a class body: the census never makes an instance field
    // name a constant method (`proto_census::ProtoCensus::fields`), so the
    // add names nothing a binding site relies on and needs no shadow
    // admission. A receiver already marked as a prototype keeps the full
    // publication (its member slots are what holder tests prove).
    let meta = crate::object::header::MetaObj::runtime_born(obj as *mut _).meta();
    if meta.is_null() || (*meta).flags & crate::object::OBJECT_META_FLAG_IS_PROTOTYPE == 0 {
        shapes::stamp_birth_shape_id_with_carrier_note(obj, target);
    } else {
        shapes::stamp_object_shape_id_with_carrier_note(obj, target);
    }
    // Capacity and representation were proved above. This store cannot collect.
    let stored = crate::object::shape_store::store_proven_own_slot(obj, to, slot as usize, value);
    debug_assert!(stored);
    stored
}

pub(super) unsafe fn predecessor(receiver: f64) -> LayoutWord {
    LayoutWord(receiver_record(receiver).map_or(0, |(obj, _)| (*obj).shape_word()))
}

/// Only a completed generic append may teach an edge. Immutable prefixes
/// prevent stable-ID mutable lists from changing its meaning later. IDs are
/// never reused; a pruned target simply misses on the next construction.
#[cold]
#[inline(never)]
pub(super) unsafe fn learn(predecessor: LayoutWord, receiver: f64, key: f64) {
    let Some((obj, _)) = receiver_record(receiver) else {
        return;
    };
    let target = LayoutWord((*obj).shape_word());
    if target == predecessor {
        return;
    }
    let (Some(from), Some(to)) = (
        shapes::shape_descriptor_of_receiver((predecessor).0),
        shapes::shape_descriptor_of_receiver((target).0),
    ) else {
        return;
    };
    let count = from.logical_key_count;
    if to.logical_key_count != count + 1
        || from.object_kind != to.object_kind
        || from.proto_id != to.proto_id
        || from.integrity_flags != 0
        || to.integrity_flags != 0
        || from.summary != 0
        || to.summary != 0
        || from.hole_count != 0
        || to.hole_count != 0
        || from.brands() != to.brands()
        || (from.keys != 0
            && !shapes::keys_prefix_is_immutable(from.keys as *const crate::ArrayHeader))
        || !shapes::keys_prefix_is_immutable(to.keys as *const crate::ArrayHeader)
    {
        return;
    }
    let (to_words, to_len) =
        crate::object::keys_array_dense_slots_resolved(to.keys as *const crate::ArrayHeader);
    if to_len <= count as usize {
        return;
    }
    let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
    let Some(bytes) =
        crate::string::js_string_key_bytes(crate::JSValue::from_bits(key.to_bits()), &mut scratch)
    else {
        return;
    };
    if bytes.is_empty()
        || bytes[0] == b'#'
        || bytes[0].is_ascii_digit()
        || !crate::string::js_string_key_matches_bytes(
            crate::JSValue::from_bits((*to_words.add(count as usize)).to_bits()),
            bytes,
        )
    {
        return;
    }
    if count != 0 {
        let (from_words, from_len) =
            crate::object::keys_array_dense_slots_resolved(from.keys as *const crate::ArrayHeader);
        if from_len < count as usize {
            return;
        }
        for i in 0..count as usize {
            let old = (*from_words.add(i)).to_bits();
            let next = (*to_words.add(i)).to_bits();
            if old != next {
                let mut prefix_scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
                let Some(bytes) = crate::string::js_string_key_bytes(
                    crate::JSValue::from_bits(old),
                    &mut prefix_scratch,
                ) else {
                    return;
                };
                if !crate::string::js_string_key_matches_bytes(
                    crate::JSValue::from_bits(next),
                    bytes,
                ) {
                    return;
                }
            }
        }
    }
    if let Some(record) = shapes::shape_record_of_receiver((predecessor).0) {
        record.note_field_define_target(target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn birth() -> f64 {
        crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc_with_parent(0x512f, 0, 4) as i64
        )
    }
    unsafe fn key(name: &[u8]) -> f64 {
        crate::value::js_nanbox_string(crate::string::intern_ascii_literal(name) as i64)
    }

    #[test]
    fn warmed_definitions_follow_shape_edges_and_type_changes_decline() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            let (a, b) = (key(b"edge_a"), key(b"edge_b"));
            let first = birth();
            crate::object::js_class_field_add(first, a, 1.0);
            crate::object::js_class_field_add(first, b, 2.0);
            let expected = predecessor(first);
            let second = birth();
            assert!(
                try_define(second, a, 3.0),
                "first field must use the class-owned edge"
            );
            assert!(
                try_define(second, b, 4.0),
                "second field must use the next edge"
            );
            assert_eq!(predecessor(second), expected);
            let different = birth();
            assert!(
                !try_define(different, a, key(b"a string")),
                "numeric successor must decline a string"
            );
            assert!(
                try_define(different, a, 5.0),
                "decline must leave membership untouched"
            );
        }
    }

    #[test]
    fn all_thirty_two_fields_reuse_forward_edges_across_key_backing_growth() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            let keys: Vec<_> = (0..32)
                .map(|i| key(format!("growth_field_{i}").as_bytes()))
                .collect();
            let birth = || {
                crate::value::js_nanbox_pointer(crate::object::js_object_alloc_with_parent(
                    0x5130, 0, 32,
                ) as i64)
            };
            let first = birth();
            for (i, &key) in keys.iter().enumerate() {
                crate::object::js_class_field_add(first, key, i as f64);
            }
            let second = birth();
            for (i, &key) in keys.iter().enumerate() {
                assert!(
                    try_define(second, key, i as f64),
                    "field {i} must reuse its forward edge"
                );
            }
            assert_eq!(predecessor(first), predecessor(second));
        }
    }

    #[test]
    fn wrong_key_and_pruned_or_unrelated_successor_decline() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            let a = key(b"edge_negative");
            let first = birth();
            crate::object::js_class_field_add(first, a, 1.0);
            let second = birth();
            let from = shapes::shape_record_of_receiver((predecessor(second)).0).unwrap();
            assert!(
                try_define(second, a, 2.0),
                "control must first witness a hit"
            );
            let third = birth();
            assert!(!try_define(third, key(b"different_key"), 3.0));
            from.note_field_define_target(LayoutWord(u32::MAX));
            assert!(
                !try_define(third, a, 3.0),
                "a weak target may have been pruned"
            );
            // An existing unrelated shape is as invalid as a retired ID.
            from.note_field_define_target(predecessor(third));
            assert!(!try_define(third, a, 3.0));
            crate::object::js_class_field_add(third, a, 3.0);
            assert!(
                try_define(birth(), a, 4.0),
                "generic definition reteaches the edge"
            );
        }
    }

    #[test]
    fn nonextensible_receiver_cannot_follow_a_warmed_edge() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            let a = key(b"edge_no_extend");
            crate::object::js_class_field_add(birth(), a, 1.0);
            let second = birth();
            assert!(try_define(birth(), a, 2.0));
            crate::object::js_object_prevent_extensions(second);
            assert!(!try_define(second, a, 2.0));
        }
    }
}
