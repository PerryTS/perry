//! DefineField of a class field on a family-born instance
//! (`js_class_field_add`): the field's definition fills its layout slot.

use super::{desc_position_by_bytes, family_desc_of, family_slot_word, later_late_key_present};
use crate::object::ObjectHeader;

/// DefineField (`js_class_field_add`) of string key `key` on family object
/// `receiver` whose layout names the key and whose slot for it holds the
/// hole: the field's definition fills the slot in place, like the store
/// probe's hole fill (`layout_probe::hole_fill_admitted`) but without its
/// chain test, since DefineField defines an own property and never consults
/// the prototype chain. The receiver alone decides: extensible, no
/// descriptors, no overflow, not a prototype, and no later key of the list
/// present (the fill keeps the key order). `false` changes nothing; the
/// caller defines the property.
///
/// # Safety
/// As [`super::family_store`].
pub(crate) unsafe fn family_define_field(receiver: f64, key: f64, value: f64) -> bool {
    let bits = receiver.to_bits();
    let key_value = crate::JSValue::from_bits(key.to_bits());
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG || !key_value.is_any_string()
    {
        return false;
    }
    let obj = (bits & crate::value::POINTER_MASK) as *mut ObjectHeader;
    let Some(d) = family_desc_of(obj) else {
        return false;
    };
    if crate::object::overflow::has_overflow(obj)
        || d.slot_count > crate::object::object_inline_capacity(obj)
    {
        return false;
    }
    let gc = crate::gc::header_from_trusted_user_ptr(obj.cast());
    if (*gc)._reserved
        & (crate::gc::OBJ_FLAG_FROZEN
            | crate::gc::OBJ_FLAG_SEALED
            | crate::gc::OBJ_FLAG_NO_EXTEND
            | crate::gc::OBJ_FLAG_STABLE_TOMBSTONES
            | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS)
        != 0
        || (crate::object::proto_validity::any_prototype_marked()
            && crate::object::proto_validity::object_is_marked_prototype(obj as usize))
    {
        return false;
    }
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(bytes) = crate::string::js_string_key_bytes(key_value, &mut sso) else {
        return false;
    };
    let Some(pos) = desc_position_by_bytes(d, bytes) else {
        return false;
    };
    let slot = (*d.keys.add(pos as usize)).slot;
    if family_slot_word(obj, slot) != crate::value::TAG_HOLE {
        return false;
    }
    let Some(record) = crate::object::shapes::object_shape_record(obj) else {
        return false;
    };
    let keys = record.keys() as usize as *const crate::array::ArrayHeader;
    if later_late_key_present(obj, keys, record.logical_key_count(), pos) {
        return false;
    }
    crate::object::store_object_field_slot(obj, slot as usize, value.to_bits());
    true
}
