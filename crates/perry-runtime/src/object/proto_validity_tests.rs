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

