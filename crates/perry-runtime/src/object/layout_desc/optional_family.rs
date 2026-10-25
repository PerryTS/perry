//! A closed optional-slot layout is a family independently of class identity.
//! Literal families keep Object.prototype and therefore class_id zero.
//! The immutable descriptor, not a learned receiver fact, owns this proof.
use super::PerryLayoutDesc;

#[inline]
pub(super) unsafe fn is_family(d: &PerryLayoutDesc) -> bool {
    if d.class_id != 0 {
        return true;
    }
    d.variant == 0
        && d.kind == 0
        && d.integrity == 0
        && d.key_count != 0
        && d.key_count == d.slot_count
        && !d.keys.is_null()
        && std::slice::from_raw_parts(d.keys, d.key_count as usize)
            .iter()
            .all(|key| key.attrs & crate::codegen_abi::LAYOUT_KEY_OPTIONAL != 0)
}
