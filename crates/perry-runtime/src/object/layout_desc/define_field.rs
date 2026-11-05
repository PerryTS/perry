//! A definition that fills a family-born instance's layout slot: DefineField
//! of a class field (`js_class_field_add`), and a constructor's
//! `Object.defineProperty(this, k, {...})` of a key the layout carries with
//! the definition's attributes.

use super::store::{family_add_kind, fill_in_order, FamilyAdd};
use super::{family_desc_of, LAYOUT_KEY_LATE};
use crate::object::ObjectHeader;

/// DefineField (`js_class_field_add`) of string key `key` on family object
/// `receiver` whose layout names the key and whose slot for it holds the
/// hole: the field's definition fills the slot in place, like the store
/// probe's hole fill (`layout_probe::hole_fill_admitted`) but without its
/// chain test, since DefineField defines an own property and never consults
/// the prototype chain. The receiver alone decides ([`fill_defined`]); a
/// class field is a plain data property, so only a key whose layout byte is
/// plain (`LAYOUT_KEY_LATE`) is filled. `false` changes nothing; the caller
/// defines the property.
///
/// # Safety
/// As [`super::family_store`].
pub(crate) unsafe fn family_define_field(receiver: f64, key: f64, value: f64) -> bool {
    let Some(obj) = layout_receiver(receiver, key) else {
        return false;
    };
    if family_desc_of(obj).is_none() {
        return false;
    }
    let gc = crate::gc::header_from_trusted_user_ptr(obj.cast());
    (*gc)._reserved & crate::gc::OBJ_FLAG_HAS_DESCRIPTORS == 0
        && fill_defined(obj, key, LAYOUT_KEY_LATE, value)
}

/// `[[DefineOwnProperty]]` of DATA descriptor `descriptor` (its absent fields
/// `false`, the key being new) of an absent string key `key` on object `obj`:
/// when the layout names the key with exactly the definition's
/// attribute byte (`LAYOUT_KEY_OPTIONAL | entry`, a constructor's literal
/// `Object.defineProperty(this, k, {...})`, perry-codegen `layout_families`)
/// and its slot holds the hole, the definition fills the slot in place. The
/// object keeps its layout: the key list already carries the key with that
/// entry, presence is the slot. Any other definition (other attributes, an
/// accessor, a key out of order) is the ordinary one. The fill sets no
/// header bit: the shape carries the key's attributes (its summary and
/// entry, what every attribute reader checks), as for an object born with
/// them (`mark_layout_object_facts`), so field guards keep their fast path.
///
/// # Safety
/// As [`super::family_store`].
/// `obj`'s key `key` is a hole (`key_view::own_key_is_hole`).
#[inline(never)]
pub(super) unsafe fn family_define_own(
    obj: *mut ObjectHeader,
    key: f64,
    descriptor: &crate::object::object_ops::DescView<'_>,
) -> bool {
    use crate::object::key_attrs::{
        ENTRY_NON_CONFIGURABLE, ENTRY_NON_ENUMERABLE, ENTRY_NON_WRITABLE,
    };
    use crate::object::object_ops::{
        DESC_CONFIGURABLE, DESC_ENUMERABLE, DESC_GET, DESC_SET, DESC_VALUE, DESC_WRITABLE,
    };
    if descriptor.has(DESC_GET)
        || descriptor.has(DESC_SET)
        || !crate::JSValue::from_bits(key.to_bits()).is_any_string()
    {
        return false;
    }
    // Each attribute: a literal `true` clears its bit, an absent field or a
    // literal `false` sets it (the key is new); any other value takes the
    // ordinary definition, which converts it.
    let mut entry = 0u8;
    for (field, bit) in [
        (DESC_WRITABLE, ENTRY_NON_WRITABLE),
        (DESC_ENUMERABLE, ENTRY_NON_ENUMERABLE),
        (DESC_CONFIGURABLE, ENTRY_NON_CONFIGURABLE),
    ] {
        let bits = if descriptor.has(field) {
            descriptor.read(field).bits()
        } else {
            crate::value::TAG_FALSE
        };
        if bits == crate::value::TAG_FALSE {
            entry |= bit;
        } else if bits != crate::value::TAG_TRUE {
            return false;
        }
    }
    let value = if descriptor.has(DESC_VALUE) {
        descriptor.read(DESC_VALUE).bits()
    } else {
        crate::value::TAG_UNDEFINED
    };
    fill_defined(obj, key, LAYOUT_KEY_LATE | entry, f64::from_bits(value))
}

/// The object a NaN-boxed `receiver` points at, when `key` is a string.
/// [`family_add_kind`] decides whether its layout names the key.
#[inline(always)]
unsafe fn layout_receiver(receiver: f64, key: f64) -> Option<*mut ObjectHeader> {
    let bits = receiver.to_bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG
        || !crate::JSValue::from_bits(key.to_bits()).is_any_string()
    {
        return None;
    }
    Some((bits & crate::value::POINTER_MASK) as *mut ObjectHeader)
}

/// Fill the hole of object `obj`'s key `key` (a string) with `value` when
/// its layout names the key with attribute byte `attrs` ([`family_add_kind`]:
/// a family layout or a layout with optional keys, the slot the hole) and
/// the receiver takes the key: extensible, and the add in order
/// ([`fill_in_order`], the rule `[[Set]]`'s add fills by). `false` changes
/// nothing.
unsafe fn fill_defined(obj: *mut ObjectHeader, key: f64, attrs: u8, value: f64) -> bool {
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(bytes) =
        crate::string::js_string_key_bytes(crate::JSValue::from_bits(key.to_bits()), &mut sso)
    else {
        return false;
    };
    let Some(FamilyAdd::Late {
        pos,
        slot,
        attrs: a,
    }) = family_add_kind(obj, bytes)
    else {
        return false;
    };
    let gc = crate::gc::header_from_trusted_user_ptr(obj.cast());
    (*gc)._reserved
        & (crate::gc::OBJ_FLAG_FROZEN
            | crate::gc::OBJ_FLAG_SEALED
            | crate::gc::OBJ_FLAG_NO_EXTEND
            | crate::gc::OBJ_FLAG_STABLE_TOMBSTONES)
        == 0
        && fill_in_order(obj, pos, slot, a == attrs, value.to_bits()).is_ok()
}
