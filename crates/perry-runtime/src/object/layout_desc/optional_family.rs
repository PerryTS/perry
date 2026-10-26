//! A nominal family is class-owned; a classless family is literal-section-owned.
//! Validate optional-slot geometry once at first publication.
use super::PerryLayoutDesc;

#[inline]
pub(super) unsafe fn is_family(d: &PerryLayoutDesc) -> bool {
    d.class_id != 0 || super::literal_birth::contains(d)
}

pub(super) unsafe fn valid_literal(d: &PerryLayoutDesc) -> bool {
    d.class_id == 0
        && d.parent_class_id == 0
        && d.proto == super::LAYOUT_PROTO_OBJECT
        && d.variant == 0
        && d.kind == 0
        && d.integrity == 0
        && d.key_count != 0
        && d.key_count == d.slot_count
        && !d.keys.is_null()
        && std::slice::from_raw_parts(d.keys, d.key_count as usize)
            .iter()
            .all(|key| key.attrs & crate::codegen_abi::LAYOUT_KEY_OPTIONAL != 0)
}
