//! The key-add memo: its ABI with the emitted hit, and the rules it rests on.
use super::*;
use std::sync::atomic::Ordering;

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

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn interned(name: &[u8]) -> *const crate::StringHeader {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::string::js_string_intern(key, fnv1a(name))
}

/// A JSON-parsed object: class-less, birth-marked ordinary, and sharing its
/// ShapeId with every other parse of the same key list.
fn parsed(src: &[u8]) -> f64 {
    let text = crate::string::js_string_from_bytes(src.as_ptr(), src.len() as u32);
    let value = unsafe { crate::json::js_json_parse(text) };
    assert!(value.is_pointer(), "JSON.parse must yield an object");
    f64::from_bits(value.bits())
}

fn stamp(value: f64) -> u32 {
    let obj = (value.to_bits() & POINTER_MASK) as *const crate::ObjectHeader;
    unsafe { crate::object::shapes::object_shape_stamp(obj) }
}

/// A site that lives as long as the process, as an emitted one does: the
/// runtime registers primed sites and walks them after every full trace.
fn leaked_site() -> &'static PackedSetSite {
    Box::leak(Box::new(PackedSetSite::empty()))
}

fn miss(
    site: &'static PackedSetSite,
    target: f64,
    key: *const crate::StringHeader,
    value: f64,
) -> f64 {
    let mut cache_slot: super::super::packed_set::PackedSetWaysSlot = std::ptr::null_mut();
    js_put_value_set_packed_miss(target, key, value, 0, &mut cache_slot, &site.set)
}

/// A key-add through the miss entry publishes `pre -> post` at the append
/// slot, and the memo then serves another receiver of `pre` without the
/// full `[[Set]]`, landing it on exactly the successor shape.
#[test]
fn a_key_add_publishes_a_memo_that_serves_the_next_receiver() {
    let key = interned(b"added_key");
    let first = parsed(b"{\"a\":1}");
    let second = parsed(b"{\"a\":2}");
    let pre = stamp(first);
    assert_eq!(pre, stamp(second), "one key list, one ShapeId");
    let site = leaked_site();
    miss(site, first, key, 5.0);
    let post = stamp(first);
    assert_ne!(post, pre);
    let shapes = site.add_shapes.load(Ordering::Relaxed);
    assert_eq!((shapes as u32, (shapes >> 32) as u32), (pre, post));
    assert_eq!(site.add_guard.load(Ordering::Relaxed) & ADD_SLOT_MASK, 1);
    assert_eq!(
        site.add_guard.load(Ordering::Relaxed) >> ADD_SLOT_BITS,
        add_generation()
    );
    let served = unsafe { packed_add_try(site, second, 6.0) };
    assert_eq!(
        served,
        Some(6.0),
        "the memo must serve a receiver of its pre-shape"
    );
    assert_eq!(
        stamp(second),
        post,
        "the served add lands on the memo's successor"
    );
}

/// Any move of either verdict word refuses the memo: an inherited setter or
/// non-writable property installed since the prime moves one of them.
#[test]
fn a_moved_generation_refuses_the_memo() {
    let key = interned(b"added_gen");
    let first = parsed(b"{\"g\":1}");
    let second = parsed(b"{\"g\":2}");
    let site = leaked_site();
    miss(site, first, key, 1.0);
    assert_ne!(site.add_shapes.load(Ordering::Relaxed), PACKED_SET_EMPTY);
    crate::object::proto_validity::bump_proto_validity();
    let pre = stamp(second);
    assert_eq!(unsafe { packed_add_try(site, second, 2.0) }, None);
    assert_eq!(stamp(second), pre, "a refused memo changes nothing");
}

/// The site owns both ShapeIds it may stamp: a full trace's carrier
/// recompute re-notes them, so an intermediate shape no live object carries
/// cannot be retired under the site.
#[test]
fn a_published_memo_owns_both_shapes_across_a_full_trace() {
    let key = interned(b"added_own");
    let first = parsed(b"{\"o\":1}");
    let site = leaked_site();
    let pre = stamp(first);
    miss(site, first, key, 1.0);
    let post = stamp(first);
    let carrier = |id: u32| {
        crate::object::shapes::shape_descriptor_by_id(id)
            .expect("descriptor exists")
            .cache_carrier
    };
    crate::object::shapes::clear_all_cache_carriers();
    assert!(
        !carrier(post),
        "the clear must reset the bit this test asserts"
    );
    note_packed_add_carriers();
    assert!(
        carrier(pre) && carrier(post),
        "a published memo must own its pre- and post-shape"
    );
}
