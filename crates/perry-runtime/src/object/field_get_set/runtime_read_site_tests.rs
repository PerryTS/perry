//! A runtime read answers `Get(O, key)` exactly as the spec does through
//! every change: an own key, an inherited key, a deleted inherited key, a
//! replaced inherited value, an own key that shadows, a `setPrototypeOf`.
//! Nothing is remembered between reads, so each read is checked against the
//! answer, and the non-observable [`probe`] against [`get`].

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
    set(scope, &obj, key, value);
    obj
}

fn set(scope: &RuntimeHandleScope, obj: &RuntimeHandle<'_>, key: &[u8], value: f64) {
    let k = scope.root_string_ptr(key_of(key));
    obj.with_mut_ptr(|o| {
        k.with_const_ptr(|kp| crate::object::js_object_set_field_by_name(o, kp, value))
    });
}

/// `Object.create(proto)`, plus an own `pad` key so the receiver has a key list.
fn inheriting<'s>(scope: &'s RuntimeHandleScope, proto: &RuntimeHandle<'_>) -> RuntimeHandle<'s> {
    let o = crate::object::js_object_create(proto.with_mut_ptr(|p: *mut ObjectHeader| boxed(p)));
    let obj = scope.root_raw_mut_ptr(unboxed(o));
    set(scope, &obj, b"pad", 1.0);
    obj
}

/// `get`, checked against `probe` whenever the probe answers.
fn read(obj: &RuntimeHandle<'_>) -> f64 {
    let probed = obj.with_mut_ptr(|o: *mut ObjectHeader| unsafe { probe(o, Key::Name(KEY)) });
    let got = obj.with_mut_ptr(|o: *mut ObjectHeader| unsafe { get(o, KEY) });
    if let Some(Answer::Data(bits)) = probed {
        assert_eq!(bits, got.to_bits(), "the probe and the read disagree");
    }
    got
}

fn is_undefined(v: f64) -> bool {
    v.to_bits() == crate::value::TAG_UNDEFINED
}

#[test]
fn an_own_key_is_read_and_follows_stores() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    let o = object_with(&scope, KEY, 41.0);
    assert_eq!(read(&o), 41.0);
    set(&scope, &o, KEY, 42.0);
    assert_eq!(read(&o), 42.0);
    let k = scope.root_string_ptr(key_of(KEY));
    o.with_mut_ptr(|p| k.with_const_ptr(|kp| crate::object::js_object_delete_field(p, kp)));
    assert!(is_undefined(read(&o)), "a deleted own key reads undefined");
}

#[test]
fn an_inherited_key_is_read_and_follows_the_holder() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    let proto = object_with(&scope, KEY, 7.0);
    let o = inheriting(&scope, &proto);
    assert_eq!(read(&o), 7.0);
    set(&scope, &proto, KEY, 8.0);
    assert_eq!(read(&o), 8.0, "a replaced holder value is read");
    let k = scope.root_string_ptr(key_of(KEY));
    proto.with_mut_ptr(|p| k.with_const_ptr(|kp| crate::object::js_object_delete_field(p, kp)));
    assert!(
        is_undefined(read(&o)),
        "a deleted holder key reads undefined"
    );
}

#[test]
fn an_own_key_added_later_shadows_the_holder() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    let proto = object_with(&scope, KEY, 7.0);
    let o = inheriting(&scope, &proto);
    assert_eq!(read(&o), 7.0);
    set(&scope, &o, KEY, 99.0);
    assert_eq!(read(&o), 99.0);
}

#[test]
fn set_prototype_of_is_seen() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    let proto = object_with(&scope, KEY, 7.0);
    let other = object_with(&scope, KEY, 9.0);
    let o = inheriting(&scope, &proto);
    assert_eq!(read(&o), 7.0);
    crate::object::js_object_set_prototype_of(
        o.with_mut_ptr(|p: *mut ObjectHeader| boxed(p)),
        other.with_mut_ptr(|p: *mut ObjectHeader| boxed(p)),
    );
    assert_eq!(read(&o), 9.0);
    crate::object::js_object_set_prototype_of(
        o.with_mut_ptr(|p: *mut ObjectHeader| boxed(p)),
        f64::from_bits(crate::value::TAG_NULL),
    );
    assert!(is_undefined(read(&o)), "a null prototype: absent");
}

#[test]
fn an_absent_key_reads_undefined() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    let o = object_with(&scope, b"other", 1.0);
    assert!(is_undefined(read(&o)));
}
