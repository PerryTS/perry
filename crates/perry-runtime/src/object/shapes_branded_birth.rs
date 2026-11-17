//! Declared WeakMap/WeakSet birth layouts, installed at agent creation.
//! These ids name immutable scalar facts and are retained by the layout,
//! just like the intrinsic function layouts. Nothing is learned at use.
use super::*;
use crate::codegen_abi::{LayoutWord, ShapeId};

// The first plain granule after the compiler's reserved static band.
pub(crate) const WEAKMAP_BIRTH_LAYOUT: u32 = SHAPE_ID_BASE + (4 << 20);
pub(crate) const WEAKSET_BIRTH_LAYOUT: u32 = WEAKMAP_BIRTH_LAYOUT + 1;
const _: () = assert!(WEAKMAP_BIRTH_LAYOUT == crate::codegen_abi::WEAKMAP_BIRTH_LAYOUT);
const _: () = assert!(WEAKSET_BIRTH_LAYOUT == crate::codegen_abi::WEAKSET_BIRTH_LAYOUT);
/// The ShapeId of the declared empty ordinary layout
/// (`codegen_abi::EMPTY_ORDINARY_BIRTH_LAYOUT_WORD`).
pub(crate) const EMPTY_ORDINARY_BIRTH_LAYOUT: u32 = WEAKSET_BIRTH_LAYOUT + 1;
pub(crate) const NATIVE_BIRTH_LAYOUT_END: u32 = EMPTY_ORDINARY_BIRTH_LAYOUT + 1;
const _: () = assert!(
    crate::codegen_abi::EMPTY_ORDINARY_TABLE_WORD
        == crate::codegen_abi::EMPTY_ORDINARY_BIRTH_LAYOUT_WORD
            | crate::object::overflow::SHADOW_BIT
);
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
            let word = LayoutWord(if id == WEAKMAP_BIRTH_LAYOUT {
                crate::codegen_abi::WEAKMAP_BIRTH_LAYOUT_WORD
            } else {
                crate::codegen_abi::WEAKSET_BIRTH_LAYOUT_WORD
            });
            let id = unsafe { self.slab_mut().publish_static(ShapeId(id), word, record) }
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
            inner.facts_append_fresh(facts, LayoutWord(id));
            inner.family_append_fresh(crate::object::canonical_keys::ROOT_NODE, 0, LayoutWord(id));
        }
        // The empty ordinary layout of Object.prototype: the facts of a
        // keyless ordinary birth, so the ordinary intern finds this record and
        // every such object carries one declared word.
        let rep = super::super::field_rep::REP_ANY;
        let mut record = ShapeRecord::new(0, 0, 0, 0, ShapeObjectKind::Ordinary, 0)
            .with_proto_id(PROTO_ID_DEFAULT)
            .with_special_facts(rep, &[], &[])
            .with_keys_node(crate::object::canonical_keys::ROOT_NODE);
        record.set(RECORD_FLAG_EXTERNAL_CARRIER, true);
        let word = LayoutWord(crate::codegen_abi::EMPTY_ORDINARY_BIRTH_LAYOUT_WORD);
        let id = unsafe {
            self.slab_mut()
                .publish_static(ShapeId(EMPTY_ORDINARY_BIRTH_LAYOUT), word, record)
        }
        .expect("empty ordinary layout publication")
        .word;
        let facts = shapes_store::integrity_facts_key(
            shapes_store::facts_key_proto_with_special(
                0,
                0,
                0,
                0,
                ShapeObjectKind::Ordinary,
                0,
                PROTO_ID_DEFAULT,
                0,
                rep,
                &[],
                &[],
            ),
            0,
        );
        inner.facts_append_fresh(facts, id);
        inner.family_append_fresh(crate::object::canonical_keys::ROOT_NODE, 0, id);
    }
}

#[cfg(test)]
#[path = "shapes_branded_birth_tests.rs"]
mod tests;

#[cfg(test)]
mod empty_ordinary_birth_tests {
    use crate::codegen_abi::{EMPTY_ORDINARY_BIRTH_LAYOUT_WORD, EMPTY_ORDINARY_TABLE_WORD};
    use crate::object::shapes::LayoutWord;

    fn record(word: u32) -> super::ShapeRecord {
        let r = super::ShapeSlab::agent_record_for_word_present(LayoutWord(word)).expect("present");
        // SAFETY: a present record of this agent, read immediately.
        unsafe { *r }
    }

    /// Premise P1a: every keyless plain birth whose [[Prototype]] is
    /// Object.prototype (kind `Ordinary`, no keys, no live slot) returns the
    /// seeded record; a table installed on it gives the table word. An
    /// unmarked runtime birth (kind `OrdinaryUnmarked`) has other facts and
    /// so another row.
    #[test]
    fn keyless_ordinary_births_carry_the_declared_empty_word() {
        let seed = record(EMPTY_ORDINARY_BIRTH_LAYOUT_WORD);
        assert_eq!(
            (seed.keys, seed.live_inline_slot_count, seed.proto_id),
            (0, 0, 0)
        );
        for _ in 0..3 {
            let obj = crate::object::alloc_basic::object_alloc_plain(0);
            assert_eq!(
                unsafe { (*obj).parent_class_id },
                EMPTY_ORDINARY_BIRTH_LAYOUT_WORD
            );
        }
        let unmarked = crate::object::js_object_alloc(0, 0);
        assert_ne!(
            unsafe { (*unmarked).parent_class_id },
            EMPTY_ORDINARY_BIRTH_LAYOUT_WORD
        );
        let obj = crate::object::alloc_basic::object_alloc_plain(0);
        let t = unsafe { crate::object::overflow::start_empty(obj, 0) };
        unsafe { crate::object::overflow::install(obj, t) };
        assert_eq!(unsafe { (*obj).parent_class_id }, EMPTY_ORDINARY_TABLE_WORD);
    }
}
