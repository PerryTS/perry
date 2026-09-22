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

#[test]
fn a_structural_mutation_of_a_marked_object_bumps_validity() {
    let _scope = Scope::new();
    unsafe {
        let proto = crate::object::js_object_alloc(0, 4);
        set(proto, "pv_bump_a", 1.0);
        mark_object_as_prototype(proto as usize);

        let before = proto_validity();
        set(proto, "pv_bump_b", 2.0);
        assert!(
            proto_validity() > before,
            "a key added to an object somebody inherits from is invisible to \
             every instance; the counter is the only thing that sees it"
        );
    }
}

#[test]
fn a_structural_mutation_of_an_unmarked_object_does_not_bump_validity() {
    let _scope = Scope::new();
    unsafe {
        // Arm the latch with an unrelated object, so this test measures the
        // per-object gate rather than the global one.
        let marked = crate::object::js_object_alloc(0, 4);
        set(marked, "pv_gate_m", 1.0);
        mark_object_as_prototype(marked as usize);

        let plain = crate::object::js_object_alloc(0, 4);
        let before = proto_validity();
        set(plain, "pv_gate_a", 1.0);
        set(plain, "pv_gate_b", 2.0);
        set(plain, "pv_gate_c", 3.0);
        assert_eq!(
            proto_validity(),
            before,
            "building an ordinary object is the common case of all mutation; \
             if it invalidated, the cache would be a recompute"
        );
    }
}

#[test]
fn a_plain_value_store_on_a_marked_object_does_not_bump_validity() {
    let _scope = Scope::new();
    unsafe {
        let proto = crate::object::js_object_alloc(0, 4);
        set(proto, "pv_store_a", 1.0);
        mark_object_as_prototype(proto as usize);

        let before = proto_validity();
        set(proto, "pv_store_a", 99.0);
        assert_eq!(
            proto_validity(),
            before,
            "a cache entry records (holder, slot) and LOADS the value on every \
             hit, so a replaced value needs no invalidation at all"
        );
    }
}

#[test]
fn the_semantic_property_epoch_moves_the_validity_word_too() {
    let _scope = Scope::new();
    let before = proto_validity();
    crate::object::prop_plan::prop_plan_epoch_bump();
    assert!(
        proto_validity() > before,
        "folding the semantic epoch into this word is what lets a cached \
         inherited entry re-prove itself with one load and one compare"
    );
}

#[test]
fn a_descriptor_install_on_a_marked_object_bumps_validity() {
    let _scope = Scope::new();
    unsafe {
        let proto = crate::object::js_object_alloc(0, 4);
        set(proto, "pv_desc_a", 1.0);
        mark_object_as_prototype(proto as usize);

        let before = proto_validity();
        crate::object::descriptor_state::set_accessor_descriptor(
            proto as usize,
            "pv_desc_a".to_string(),
            crate::object::descriptor_state::AccessorDescriptor { get: 0, set: 0 },
        );
        assert!(
            proto_validity() > before,
            "redefining an inherited data property as an accessor changes what \
             the read must DO, not just what it returns"
        );
    }
}

#[test]
fn set_prototype_of_on_a_marked_object_bumps_validity() {
    let _scope = Scope::new();
    unsafe {
        let grand = crate::object::js_object_alloc(0, 4);
        set(grand, "pv_spo_g", 5.0);
        let proto = crate::object::js_object_alloc(0, 4);
        set(proto, "pv_spo_a", 1.0);
        mark_object_as_prototype(proto as usize);

        let before = proto_validity();
        let boxed = |o: *mut crate::object::ObjectHeader| {
            f64::from_bits(crate::value::js_nanbox_pointer(o as i64).to_bits())
        };
        crate::object::js_object_set_prototype_of(boxed(proto), boxed(grand));
        assert!(
            proto_validity() > before,
            "re-parenting an interior prototype changes what every instance \
             below it resolves, and touches none of them"
        );
    }
}

#[test]
fn deleting_a_key_from_a_marked_object_bumps_validity() {
    let _scope = Scope::new();
    unsafe {
        let proto = crate::object::js_object_alloc(0, 4);
        set(proto, "pv_del_a", 1.0);
        set(proto, "pv_del_b", 2.0);
        mark_object_as_prototype(proto as usize);

        let before = proto_validity();
        crate::object::js_object_delete_field(proto, key("pv_del_b"));
        assert!(
            proto_validity() > before,
            "a stable-tombstone delete deliberately keeps the ShapeId, so the \
             shape hook cannot see it; the semantic epoch half must"
        );
    }
}
