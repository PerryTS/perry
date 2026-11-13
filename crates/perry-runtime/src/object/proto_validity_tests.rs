//! The validity word's two halves, tested separately.
//!
//! A counter that is bumped by EVERYTHING is trivially correct and useless; a
//! counter that is bumped by nothing is fast and returns stale values. So each
//! test here asserts a DELTA in both directions: the events that must
//! invalidate move it, and the events that must not — a plain value store, a
//! structural mutation of an object nobody inherits from — leave it alone.

use super::*;

fn key(name: &str) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn set(obj: *mut crate::object::ObjectHeader, name: &str, value: f64) {
    crate::object::js_object_set_field_by_name(obj, key(name), value);
}

struct Scope {
    _suppress: crate::gc::GcSuppressScope,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl Scope {
    fn new() -> Self {
        Self {
            _lock: crate::gc::global_side_table_test_lock(),
            _suppress: crate::gc::GcSuppressScope::new(),
        }
    }
}

#[test]
fn a_fresh_object_is_not_marked() {
    let _scope = Scope::new();
    unsafe {
        let obj = crate::object::js_object_alloc(0, 4);
        assert!(
            !object_is_marked_prototype(obj as usize),
            "a fresh allocation's reserved word must be zero, or an address \
             the collector recycled would inherit the previous tenant's mark"
        );
    }
}

#[test]
fn marking_is_visible_and_arms_the_latch() {
    let _scope = Scope::new();
    unsafe {
        let obj = crate::object::js_object_alloc(0, 4);
        set(obj, "pv_mark_a", 1.0);
        mark_object_as_prototype(obj as usize);
        assert!(object_is_marked_prototype(obj as usize));
        assert!(
            any_prototype_marked(),
            "the latch is what lets the mutation path skip the header load \
             entirely until something is marked"
        );
    }
}

/// A runtime intrinsic holder born on the born-prototype lineage
/// (`shapes::stamp_born_prototype_root`) takes its prototype mark without a
/// shape transition: its ShapeId does not move. The control, the same build
/// without the born root, still mints its private lineage at the mark, and
/// that lineage is never the born generation.
#[test]
fn a_born_prototype_holder_is_marked_without_a_transition() {
    use crate::object::shapes::{object_on_born_prototype_lineage, object_shape_stamp};
    let _scope = Scope::new();
    unsafe {
        let holder = crate::object::object_alloc_prototype_old(0, 2);
        crate::object::shapes::stamp_born_prototype_root(holder);
        assert!(object_on_born_prototype_lineage(holder));
        set(holder, "pv_born_a", 1.0);
        assert!(
            object_on_born_prototype_lineage(holder),
            "a key add carries the born lineage"
        );
        let before = object_shape_stamp(holder);
        mark_object_as_prototype(holder as usize);
        assert!(object_is_marked_prototype(holder as usize));
        assert_eq!(
            object_shape_stamp(holder).0,
            before.0,
            "a born-lineage holder's mark mints no shape"
        );

        let plain = crate::object::object_alloc_prototype_old(0, 2);
        set(plain, "pv_born_a", 1.0);
        assert!(!object_on_born_prototype_lineage(plain));
        let before = object_shape_stamp(plain);
        mark_object_as_prototype(plain as usize);
        assert!(object_is_marked_prototype(plain as usize));
        assert_ne!(
            object_shape_stamp(plain).0,
            before.0,
            "an ordinary object's mark mints its private lineage"
        );
        assert!(
            !object_on_born_prototype_lineage(plain),
            "the counter never hands out the born generation"
        );
    }
}

/// The generator prototypes and the built-in iterator prototypes are born on
/// the born-prototype lineage, so their seals (#14165, #14185) cost no shape
/// mint at startup: each is marked, sealed, and still on that lineage.
#[test]
fn the_generator_and_iterator_prototypes_keep_their_born_lineage() {
    use crate::object::shapes::object_on_born_prototype_lineage;
    let _scope = Scope::new();
    crate::object::ensure_generator_intrinsics();
    let _ = crate::object::iterator_ctor::iterator_prototype_object();
    let mut holders = Vec::new();
    for slot in [
        &crate::object::GENERATOR_PROTOTYPE_PTR,
        &crate::object::ASYNC_GENERATOR_PROTOTYPE_PTR,
        &crate::object::iterator_prototypes::MAP_ITERATOR_PROTOTYPE_PTR,
        &crate::object::iterator_prototypes::SET_ITERATOR_PROTOTYPE_PTR,
        &crate::object::iterator_prototypes::STRING_ITERATOR_PROTOTYPE_PTR,
        &crate::object::iterator_prototypes::REGEXP_STRING_ITERATOR_PROTOTYPE_PTR,
    ] {
        holders.push(
            slot.load(std::sync::atomic::Ordering::Acquire) as *mut crate::object::ObjectHeader
        );
    }
    for holder in holders {
        assert!(!holder.is_null());
        unsafe {
            assert!(object_is_marked_prototype(holder as usize));
            let meta = crate::meta_read!(crate::meta_obj_runtime_born!(holder));
            assert!(
                !meta.is_null()
                    && (*meta).flags & crate::object::meta_flags::OBJECT_META_FLAG_SEALED_INTRINSIC
                        != 0,
                "sealed"
            );
            assert!(
                object_on_born_prototype_lineage(holder),
                "the seal took no private-lineage transition"
            );
        }
    }
}

/// The constructor loop births the prototypes the realm seals at startup
/// (`builtin_proto::realm_seals_constructor_prototype`) on the born-prototype
/// lineage, so their seal takes no private-lineage transition. Red without the
/// loop's stamp: `seal_realm` moves each one to a fresh generation. Controls: a
/// constructor prototype the realm does not seal stays on the ordinary lineage
/// and unmarked, and an ordinary object marked later still mints its own
/// private lineage, never the born one.
#[test]
fn the_realm_sealed_constructor_prototypes_keep_their_born_lineage() {
    use crate::object::shapes::{object_on_born_prototype_lineage, object_shape_descriptor};
    let _scope = Scope::new();
    let _ = crate::object::js_get_global_this();
    let proto_of = |name: &str| {
        let bits = crate::object::builtin_prototype_value(name).to_bits();
        (bits & crate::value::POINTER_MASK) as *mut crate::object::ObjectHeader
    };
    for name in [
        "String", "Number", "Map", "Set", "Promise", "WeakSet", "Error",
    ] {
        let holder = proto_of(name);
        assert!(!holder.is_null(), "{name}.prototype");
        unsafe {
            assert!(
                object_is_marked_prototype(holder as usize),
                "{name}.prototype is marked"
            );
            assert!(
                object_on_born_prototype_lineage(holder),
                "{name}.prototype: its seal took no private-lineage transition"
            );
        }
    }
    let unsealed = proto_of("TextDecoder");
    assert!(!unsealed.is_null());
    unsafe {
        assert!(
            !object_is_marked_prototype(unsealed as usize),
            "TextDecoder.prototype is not sealed"
        );
        assert!(
            !object_on_born_prototype_lineage(unsealed),
            "an unsealed prototype keeps the ordinary lineage"
        );
    }
    let ordinary = crate::object::js_object_alloc(0, 0);
    set(ordinary, "a", 1.0);
    unsafe {
        assert!(!object_on_born_prototype_lineage(ordinary));
        let _ = mark_object_as_prototype(ordinary as usize);
        assert!(object_is_marked_prototype(ordinary as usize));
        let d = object_shape_descriptor(ordinary).expect("shape");
        assert!(
            d.semantic_generation > crate::object::shapes::BORN_PROTOTYPE_GENERATION,
            "a program-marked object mints its own private lineage"
        );
    }
}
