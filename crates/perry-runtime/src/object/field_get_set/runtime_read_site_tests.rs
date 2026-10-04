//! A runtime read site answers `Get(O, key)` exactly as the spec does through
//! every change a shape fact covers: an own key, an inherited key, a deleted
//! inherited key, a replaced inherited value and a `setPrototypeOf`. Each
//! scenario also proves the site really cached (the next read is a leaf hit),
//! so the correct answers are not the slow entry's alone.

use super::*;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};

const KEY: &[u8] = b"then";

fn key_of(bytes: &[u8]) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn boxed(obj: *mut ObjectHeader) -> f64 {
    f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits())
}

fn unboxed(value: f64) -> *mut ObjectHeader {
    (value.to_bits() & crate::value::POINTER_MASK) as *mut ObjectHeader
}

/// A fresh ordinary object with `key: value`.
fn object_with<'s>(scope: &'s RuntimeHandleScope, key: &[u8], value: f64) -> RuntimeHandle<'s> {
    let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 4));
    let k = scope.root_string_ptr(key_of(key));
    obj.with_mut_ptr(|o| {
        k.with_const_ptr(|kp| crate::object::js_object_set_field_by_name(o, kp, value))
    });
    obj
}

/// `Object.create(proto)`, plus an own `pad` key so the receiver has a key list.
fn inheriting<'s>(scope: &'s RuntimeHandleScope, proto: &RuntimeHandle<'_>) -> RuntimeHandle<'s> {
    let o = crate::object::js_object_create(proto.with_mut_ptr(|p: *mut ObjectHeader| boxed(p)));
    let obj = scope.root_raw_mut_ptr(unboxed(o));
    let k = scope.root_string_ptr(key_of(b"pad"));
    obj.with_mut_ptr(|o| {
        k.with_const_ptr(|kp| crate::object::js_object_set_field_by_name(o, kp, 1.0))
    });
    obj
}

fn read(site: &RuntimeReadSite, obj: &RuntimeHandle<'_>) -> f64 {
    obj.with_mut_ptr(|o: *mut ObjectHeader| unsafe { site.read(o, KEY) })
}

fn leaf(site: &RuntimeReadSite, obj: &RuntimeHandle<'_>) -> Option<f64> {
    obj.with_mut_ptr(|o: *mut ObjectHeader| unsafe { site.read_leaf(o) })
}

fn is_undefined(v: f64) -> bool {
    v.to_bits() == crate::value::TAG_UNDEFINED
}

/// Holder entries are primed only while no worker agent exists, and that gate
/// is process-wide and sticky: each holder scenario runs in a fresh process.
macro_rules! fresh_gate {
    ($name:literal) => {
        if !crate::object::method_site::run_with_fresh_worker_gate($name) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
    };
}

#[test]
fn an_own_key_is_answered_by_the_compact_word() {
    let _lock = crate::gc::global_side_table_test_lock();
    let site = RuntimeReadSite::new();
    let scope = RuntimeHandleScope::new();
    let o = object_with(&scope, KEY, 41.0);
    assert_eq!(read(&site, &o), 41.0);
    assert_eq!(leaf(&site, &o), Some(41.0), "the slow read primes the word");
    // A value store keeps the shape: the hit loads the new value.
    let k = scope.root_string_ptr(key_of(KEY));
    o.with_mut_ptr(|p| {
        k.with_const_ptr(|kp| crate::object::js_object_set_field_by_name(p, kp, 42.0))
    });
    assert_eq!(leaf(&site, &o), Some(42.0));
    // Deleting the own key moves the receiver's ShapeId: no stale hit.
    o.with_mut_ptr(|p| k.with_const_ptr(|kp| crate::object::js_object_delete_field(p, kp)));
    assert_ne!(leaf(&site, &o), Some(42.0));
    assert!(is_undefined(read(&site, &o)));
}

#[test]
fn an_inherited_key_is_answered_by_the_holder_entry() {
    fresh_gate!("an_inherited_key_is_answered_by_the_holder_entry");
    let site = RuntimeReadSite::new();
    let scope = RuntimeHandleScope::new();
    let proto = object_with(&scope, KEY, 7.0);
    let o = inheriting(&scope, &proto);
    assert_eq!(read(&site, &o), 7.0);
    assert_eq!(
        leaf(&site, &o),
        Some(7.0),
        "the slow read primes the holder entry"
    );
    // A second receiver of the same shape is answered without priming.
    let o2 = inheriting(&scope, &proto);
    assert_eq!(leaf(&site, &o2), Some(7.0));
    // A value store into the holder keeps its shape: the hit loads it.
    let k = scope.root_string_ptr(key_of(KEY));
    proto.with_mut_ptr(|p| {
        k.with_const_ptr(|kp| crate::object::js_object_set_field_by_name(p, kp, 8.0))
    });
    assert_eq!(leaf(&site, &o), Some(8.0));
    assert_eq!(read(&site, &o), 8.0);
}

#[test]
fn a_deleted_inherited_key_is_not_answered_from_the_entry() {
    fresh_gate!("a_deleted_inherited_key_is_not_answered_from_the_entry");
    let site = RuntimeReadSite::new();
    let scope = RuntimeHandleScope::new();
    let proto = object_with(&scope, KEY, 7.0);
    let k = scope.root_string_ptr(key_of(b"other"));
    proto.with_mut_ptr(|p| {
        k.with_const_ptr(|kp| crate::object::js_object_set_field_by_name(p, kp, 3.0))
    });
    let o = inheriting(&scope, &proto);
    assert_eq!(read(&site, &o), 7.0);
    assert_eq!(leaf(&site, &o), Some(7.0));
    let then = scope.root_string_ptr(key_of(KEY));
    proto.with_mut_ptr(|p| then.with_const_ptr(|kp| crate::object::js_object_delete_field(p, kp)));
    let after = leaf(&site, &o);
    assert!(
        after.is_none() || after.is_some_and(is_undefined),
        "a deleted holder key must not be answered from the entry: {after:?}"
    );
    assert!(
        is_undefined(read(&site, &o)),
        "Get answers undefined after the delete"
    );
    // Absence is a shape fact too: the next read is a leaf answer.
    let again = leaf(&site, &o);
    assert!(again.is_none() || again.is_some_and(is_undefined));
}

#[test]
fn an_own_key_added_later_shadows_the_holder() {
    fresh_gate!("an_own_key_added_later_shadows_the_holder");
    let site = RuntimeReadSite::new();
    let scope = RuntimeHandleScope::new();
    let proto = object_with(&scope, KEY, 7.0);
    let o = inheriting(&scope, &proto);
    assert_eq!(read(&site, &o), 7.0);
    assert_eq!(leaf(&site, &o), Some(7.0));
    let k = scope.root_string_ptr(key_of(KEY));
    o.with_mut_ptr(|p| {
        k.with_const_ptr(|kp| crate::object::js_object_set_field_by_name(p, kp, 99.0))
    });
    assert_ne!(
        leaf(&site, &o),
        Some(7.0),
        "the own key moved the receiver's ShapeId"
    );
    assert_eq!(read(&site, &o), 99.0);
}

#[test]
fn set_prototype_of_is_seen_by_the_site() {
    fresh_gate!("set_prototype_of_is_seen_by_the_site");
    let site = RuntimeReadSite::new();
    let scope = RuntimeHandleScope::new();
    let proto = object_with(&scope, KEY, 7.0);
    let other = object_with(&scope, KEY, 9.0);
    let o = inheriting(&scope, &proto);
    assert_eq!(read(&site, &o), 7.0);
    assert_eq!(leaf(&site, &o), Some(7.0));
    crate::object::js_object_set_prototype_of(
        o.with_mut_ptr(|p: *mut ObjectHeader| boxed(p)),
        other.with_mut_ptr(|p: *mut ObjectHeader| boxed(p)),
    );
    assert_ne!(
        leaf(&site, &o),
        Some(7.0),
        "setPrototypeOf moved the receiver's ShapeId"
    );
    assert_eq!(read(&site, &o), 9.0);
    // A null prototype: absent.
    crate::object::js_object_set_prototype_of(
        o.with_mut_ptr(|p: *mut ObjectHeader| boxed(p)),
        f64::from_bits(crate::value::TAG_NULL),
    );
    assert!(is_undefined(read(&site, &o)));
}

#[test]
fn object_receiver_admits_only_ordinary_objects() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    let o = object_with(&scope, KEY, 1.0);
    assert!(object_receiver(o.with_mut_ptr(|p: *mut ObjectHeader| boxed(p))).is_some());
    assert!(object_receiver(1.5).is_none());
    assert!(object_receiver(f64::from_bits(crate::value::TAG_UNDEFINED)).is_none());
    let arr = crate::array::js_array_alloc(2);
    assert!(object_receiver(boxed(arr as *mut ObjectHeader)).is_none());
}
