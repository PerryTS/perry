//! Declared WeakMap/WeakSet birth layouts, installed at agent creation.
//! These ids name immutable scalar facts and are retained by the layout,
//! just like the intrinsic function layouts. Nothing is learned at use.
use super::*;

// The first plain granule after the compiler's reserved static band.
pub(crate) const WEAKMAP_BIRTH_LAYOUT: u32 = SHAPE_ID_BASE + (4 << 20);
pub(crate) const WEAKSET_BIRTH_LAYOUT: u32 = WEAKMAP_BIRTH_LAYOUT + 1;
const _: () = assert!(WEAKMAP_BIRTH_LAYOUT == crate::codegen_abi::WEAKMAP_BIRTH_LAYOUT);
const _: () = assert!(WEAKSET_BIRTH_LAYOUT == crate::codegen_abi::WEAKSET_BIRTH_LAYOUT);
pub(crate) const NATIVE_BIRTH_LAYOUT_END: u32 = WEAKSET_BIRTH_LAYOUT + 1;
const _: () = assert!(STATIC_SHAPE_ID_END <= WEAKMAP_BIRTH_LAYOUT);
const _: () = assert!(NATIVE_BIRTH_LAYOUT_END < SITE_SHAPE_ID_END);
const _: () = assert!(WEAKMAP_BIRTH_LAYOUT & SHAPE_ID_KIND_MASK == 0);

pub(crate) const fn branded_birth_layout(class_id: u32) -> u32 {
    match class_id {
        crate::weakref::CLASS_ID_WEAKMAP => crate::codegen_abi::WEAKMAP_BIRTH_LAYOUT_WORD,
        crate::weakref::CLASS_ID_WEAKSET => crate::codegen_abi::WEAKSET_BIRTH_LAYOUT_WORD,
        _ => panic!("no declared native birth layout for class"),
    }
}

impl ShapeTable {
    pub(super) fn seed_native_birth_layouts(&self) {
        let mut inner = self.inner.borrow_mut();
        for class in [
            crate::weakref::CLASS_ID_WEAKMAP,
            crate::weakref::CLASS_ID_WEAKSET,
        ] {
            let id = match class {
                crate::weakref::CLASS_ID_WEAKMAP => WEAKMAP_BIRTH_LAYOUT,
                _ => WEAKSET_BIRTH_LAYOUT,
            };
            let proto = PROTO_ID_CLASS | u64::from(class);
            let brands = [u64::from(class)];
            let rep = super::super::field_rep::REP_ANY;
            let mut record = ShapeRecord::new(0, 0, 0, 0, ShapeObjectKind::Ordinary, 0)
                .with_proto_id(proto)
                .with_special_facts(rep, &[], &brands);
            record.set(RECORD_FLAG_EXTERNAL_CARRIER, true);
            let word = crate::codegen_abi::LayoutWord(if id == WEAKMAP_BIRTH_LAYOUT {
                crate::codegen_abi::WEAKMAP_BIRTH_LAYOUT_WORD
            } else {
                crate::codegen_abi::WEAKSET_BIRTH_LAYOUT_WORD
            });
            let id = unsafe {
                self.slab_mut()
                    .publish_static(crate::codegen_abi::ShapeId(id), word, record)
            }
            .expect("weak collection layout publication")
            .word
            .0;
            let facts = shapes_store::integrity_facts_key(
                shapes_store::facts_key_proto_with_special(
                    0,
                    0,
                    0,
                    0,
                    ShapeObjectKind::Ordinary,
                    0,
                    proto,
                    0,
                    rep,
                    &[],
                    &brands,
                ),
                0,
            );
            inner.facts_append_fresh(facts, id);
            inner.family_append_fresh(0, id);
        }
    }
}

#[cfg(test)]
#[path = "shapes_branded_birth_tests.rs"]
mod tests;
