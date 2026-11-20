//! Literal-section ownership is decoded by literal_birth::family_at.
//! Validate optional-slot geometry once at first publication.
use super::PerryLayoutDesc;

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
