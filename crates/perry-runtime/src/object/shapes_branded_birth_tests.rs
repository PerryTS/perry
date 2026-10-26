use super::*;
use crate::codegen_abi::LayoutWord;

#[test]
fn declared_branded_births_keep_prototype_brand_and_fresh_identity() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    // A Rust test thread is a fresh agent; mirror executable startup.
    let _ = crate::state::state();
    for class in [
        crate::weakref::CLASS_ID_WEAKMAP,
        crate::weakref::CLASS_ID_WEAKSET,
    ] {
        let a = crate::object::object_alloc_branded(class);
        let b = crate::object::object_alloc_branded(class);
        assert_ne!(a, b);
        unsafe {
            let shape = object_shape_stamp(a);
            assert_eq!(shape, LayoutWord(branded_birth_layout(class)));
            assert_eq!(shape, object_shape_stamp(b));
            let record = shapes_store::ShapeSlab::agent_record_for_word_present(shape).unwrap();
            let record = &*record;
            assert_eq!(record.proto_id, PROTO_ID_CLASS | u64::from(class));
            assert_eq!(record.brands(), [u64::from(class)]);
            assert_eq!(record.object_kind(), ShapeObjectKind::Ordinary);
        }
    }
}

#[test]
fn declared_branded_layouts_survive_shape_table_reset() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    test_clear_shape_table();
    for class in [
        crate::weakref::CLASS_ID_WEAKMAP,
        crate::weakref::CLASS_ID_WEAKSET,
    ] {
        let id = branded_birth_layout(class);
        let record =
            shapes_store::ShapeSlab::agent_record_for_word_present(LayoutWord(id)).unwrap();
        assert_eq!(unsafe { &*record }.brands(), [u64::from(class)]);
    }
}
