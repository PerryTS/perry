//! Branded native construction sites remember a scalar ShapeId only.
//! Presence and complete facts are revalidated on every use; the interner
//! resolves misses and retired records. No constructor/property name is used.
use super::{shapes_store::ShapeSlab, *};

#[derive(Default)]
pub(crate) struct BrandedBirthSite(std::cell::Cell<u32>);

impl BrandedBirthSite {
    pub(crate) const fn new() -> Self {
        Self(std::cell::Cell::new(0))
    }

    #[inline]
    pub(crate) fn resolve(&self, class_id: u32, brand: u64) -> u32 {
        let id = self.0.get();
        if branded_birth_matches(id, class_id, brand) {
            return id;
        }
        let id = publish_shape_result(shape_descriptor_intern_with_rep(
            std::ptr::null(),
            0,
            0,
            0,
            ShapeObjectKind::Ordinary,
            0,
            PROTO_ID_CLASS | u64::from(class_id),
            0,
            super::super::field_rep::REP_ANY,
            &[brand],
            None,
            0,
        ));
        self.0.set(id);
        id
    }
}

#[inline]
pub(crate) fn branded_birth_matches(id: u32, class_id: u32, brand: u64) -> bool {
    let Some(record) = ShapeSlab::agent_record_present(id) else {
        return false;
    };
    let r = unsafe { &*record };
    r.keys == 0
        && r.logical_key_count == 0
        && r.live_inline_slot_count == 0
        && r.semantic_generation == 0
        && r.hole_count == 0
        && r.object_kind() == ShapeObjectKind::Ordinary
        && r.proto_id == (PROTO_ID_CLASS | u64::from(class_id))
        && r.summary() == 0
        && r.rep == 0
        && r.special_constfn_mask() == 0
        && r.integrity_flags() == 0
        && r.brands() == [brand]
}
#[cfg(test)]
#[path = "shapes_branded_birth_tests.rs"]
mod tests;
