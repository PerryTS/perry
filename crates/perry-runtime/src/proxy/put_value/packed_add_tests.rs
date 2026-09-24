//! The key-add memo: its ABI with the emitted hit, and the rules it rests on.
use super::*;

#[test]
fn packed_set_site_layout_matches_codegen() {
    // perry-codegen `expr/put_value_store_ic.rs`: PACKED_SET_SITE_WORDS,
    // ADD_SHAPES_WORD, ADD_GUARD_WORD, ADD_SLOT_BITS.
    assert_eq!(
        std::mem::size_of::<PackedSetSite>(),
        8 * PACKED_SET_SITE_WORDS
    );
    assert_eq!(PACKED_SET_SITE_WORDS, 4);
    assert_eq!(std::mem::offset_of!(PackedSetSite, add_ways), 24);
    assert_eq!(std::mem::offset_of!(PackedSetSite, set), 0);
    assert_eq!(
        std::mem::offset_of!(PackedSetSite, add_shapes),
        8 * ADD_SHAPES_WORD
    );
    assert_eq!(
        std::mem::offset_of!(PackedSetSite, add_guard),
        8 * ADD_GUARD_WORD
    );
    assert_eq!((ADD_SHAPES_WORD, ADD_GUARD_WORD, ADD_SLOT_BITS), (1, 2, 16));
    // An empty site's pre half is unmatchable.
    let empty = PackedSetSite::empty();
    assert!(empty.add_shapes.load(Ordering::Relaxed) as u32 >= crate::object::shapes::SHAPE_ID_END);
}

#[test]
fn packed_add_refuse_bits_match_codegen() {
    // perry-codegen ADD_REFUSE_RESERVED / ADD_REFUSE_GC_FLAGS, read as one
    // little-endian u32 at the GcHeader: obj_type | gc_flags << 8 | _reserved << 16.
    let reserved = crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
        | crate::gc::OBJ_FLAG_STABLE_TOMBSTONES
        | crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF;
    assert_eq!(reserved, 0x0C80);
    // ADD_LAYOUT_RESERVED: the states `mark_object_dynamic_shape_unknown` acts on.
    assert_eq!(
        crate::gc::GC_LAYOUT_SIDE_MASK | crate::gc::GC_OBJ_TYPED_LAYOUT_INTACT,
        0x9000
    );
    assert_eq!(crate::gc::GC_FLAG_TENURED, 0x20);
    assert_eq!(std::mem::offset_of!(crate::gc::GcHeader, obj_type), 0);
    assert_eq!(std::mem::offset_of!(crate::gc::GcHeader, gc_flags), 1);
    assert_eq!(std::mem::offset_of!(crate::gc::GcHeader, _reserved), 2);
}

#[test]
fn packed_add_meta_layout_matches_codegen() {
    // perry-codegen META_FLAGS_OFFSET / META_ELEMENTS_OFFSET / META_REFUSE_FLAGS.
    assert_eq!(std::mem::offset_of!(crate::object::ObjectMeta, flags), 24);
    assert_eq!(
        std::mem::offset_of!(crate::object::ObjectMeta, elements),
        96
    );
    assert_eq!(
        crate::object::OBJECT_META_FLAG_IS_PROTOTYPE
            | crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER,
        0x60
    );
}
