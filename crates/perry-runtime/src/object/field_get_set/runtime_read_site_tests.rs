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

extern "C" fn probe_getter(
    _c: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    panic!("a non-observable probe must never invoke the getter")
}

extern "C" fn replacement_probe_getter(
    _c: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    panic!("a replacement probe must never invoke the getter")
}

#[test]
fn probe_rechecks_accessor_lane_without_invoking_it() {
    fresh_gate!("probe_rechecks_accessor_lane_without_invoking_it");
    use crate::object::method_site::read_holder::probe::{Answer, Key};
    let site = RuntimeReadSite::new();
    let scope = RuntimeHandleScope::new();
    let proto = object_with(&scope, KEY, 7.0);
    let o = inheriting(&scope, &proto);
    let closure = crate::closure::js_closure_alloc(crate::fn_info!(probe_getter, 0), 0);
    let get = boxed(closure.cast()).to_bits();
    proto.with_mut_ptr::<ObjectHeader, _>(|p| {
        crate::object::set_accessor_descriptor(
            p as usize,
            "then".to_string(),
            crate::object::AccessorDescriptor {
                get,
                ..Default::default()
            },
        )
    });
    let probe =
        || o.with_const_ptr::<ObjectHeader, _>(|p| unsafe { site.probe(p, Key::Name(KEY)) });
    assert!(matches!(probe(), Some(Answer::Getter(bits)) if bits == get));
    assert!(matches!(probe(), Some(Answer::Getter(bits)) if bits == get));
    let code = || {
        o.with_const_ptr::<ObjectHeader, _>(|p| unsafe {
            site.probe_getter_code(p, Key::Name(KEY))
        })
    };
    assert_eq!(code(), Some(probe_getter as *const () as usize));
    // A direct replacement of the immutable pair is the lane guard's
    // witness. Public descriptor replacement currently also retires the
    // holder shape, so it cannot isolate this guard on its own.
    let _no_gc = crate::gc::GcSuppressScope::new();
    let replacement =
        crate::closure::js_closure_alloc(crate::fn_info!(replacement_probe_getter, 0), 0);
    let next = boxed(replacement.cast()).to_bits();
    let pair = unsafe {
        crate::object::accessor_pair::pair_new(crate::object::accessor_pair::Accessor {
            get: next,
            ..Default::default()
        })
    };
    let before = proto.with_const_ptr::<ObjectHeader, _>(|p| unsafe {
        crate::object::shapes::object_shape_stamp(p)
    });
    proto.with_mut_ptr::<ObjectHeader, _>(|p| unsafe {
        let keys = crate::object::object_keys(p);
        let slot =
            crate::object::keys_find_property_slot_by_bytes(keys.arr(), keys.count(), KEY).unwrap();
        crate::object::slot_store::store_object_field_slot(
            p,
            slot as usize,
            boxed(pair.cast()).to_bits(),
        );
    });
    let after = proto.with_const_ptr::<ObjectHeader, _>(|p| unsafe {
        crate::object::shapes::object_shape_stamp(p)
    });
    assert_eq!(
        before, after,
        "replacing only the pair leaves the holder shape unchanged"
    );
    assert_eq!(code(), Some(replacement_probe_getter as *const () as usize));
    assert!(matches!(probe(), Some(Answer::Getter(bits)) if bits == next));
}

#[test]
fn probe_symbol_reads_current_slot_and_declines_an_accessor() {
    fresh_gate!("probe_symbol_reads_current_slot_and_declines_an_accessor");
    use crate::object::method_site::read_holder::probe::{Answer, Key};
    let site = RuntimeReadSite::new();
    let scope = RuntimeHandleScope::new();
    let proto = object_with(&scope, KEY, 7.0);
    let o = inheriting(&scope, &proto);
    let symbol = scope.root_raw_mut_ptr(crate::symbol::well_known_symbol("replace"));
    let set = |value| {
        proto.with_mut_ptr::<ObjectHeader, _>(|p| {
            symbol.with_mut_ptr::<crate::symbol::SymbolHeader, _>(|sym| unsafe {
                crate::symbol::js_object_set_symbol_property(boxed(p), boxed(sym.cast()), value);
            })
        })
    };
    let probe = || {
        o.with_const_ptr::<ObjectHeader, _>(|p| {
            symbol.with_const_ptr::<crate::symbol::SymbolHeader, _>(|sym| unsafe {
                site.probe(p, Key::Symbol(sym as usize))
            })
        })
    };
    set(11.0);
    assert!(matches!(probe(), Some(Answer::Data(bits)) if bits == 11.0f64.to_bits()));
    set(12.0);
    assert!(matches!(probe(), Some(Answer::Data(bits)) if bits == 12.0f64.to_bits()));
    let closure = crate::closure::js_closure_alloc(crate::fn_info!(probe_getter, 0), 0);
    let get = boxed(closure.cast()).to_bits();
    proto.with_mut_ptr::<ObjectHeader, _>(|p| {
        symbol.with_const_ptr::<crate::symbol::SymbolHeader, _>(|sym| unsafe {
            crate::object::shaped_symbols::define_accessor(p as usize, sym as usize, get, 0);
        })
    });
    assert!(matches!(probe(), Some(Answer::Getter(bits)) if bits == get));
}

#[test]
fn probe_own_accessor_never_reuses_another_receivers_pair() {
    fresh_gate!("probe_own_accessor_never_reuses_another_receivers_pair");
    use crate::object::method_site::read_holder::probe::{Answer, Key};
    let site = RuntimeReadSite::new();
    let scope = RuntimeHandleScope::new();
    let a = object_with(&scope, KEY, 1.0);
    let b = object_with(&scope, KEY, 1.0);
    let mut getters = [0; 2];
    for (object, slot) in [&a, &b].into_iter().zip(getters.iter_mut()) {
        let closure = crate::closure::js_closure_alloc(crate::fn_info!(probe_getter, 0), 0);
        *slot = boxed(closure.cast()).to_bits();
        object.with_mut_ptr::<ObjectHeader, _>(|p| {
            crate::object::set_accessor_descriptor(
                p as usize,
                "then".to_string(),
                crate::object::AccessorDescriptor {
                    get: *slot,
                    ..Default::default()
                },
            )
        });
    }
    for (object, get) in [&a, &b, &a]
        .into_iter()
        .zip([getters[0], getters[1], getters[0]])
    {
        assert!(
            matches!(object.with_const_ptr::<ObjectHeader, _>(|p| unsafe { site.probe(p, Key::Name(KEY)) }),
            Some(Answer::Getter(bits)) if bits == get)
        );
    }
}

#[test]
fn constructor_probe_and_read_share_an_ordinary_serial_holder() {
    fresh_gate!("constructor_probe_and_read_share_an_ordinary_serial_holder");
    use crate::object::method_site::read_holder::probe::{Answer, Key};
    let scope = RuntimeHandleScope::new();
    let proto = object_with(&scope, b"constructor", 11.0);
    let receiver = inheriting(&scope, &proto);
    let site = RuntimeReadSite::new();
    receiver.with_mut_ptr::<ObjectHeader, _>(|p| unsafe {
        assert!(matches!(site.probe(p, Key::Name(b"constructor")),
            Some(Answer::Data(bits)) if bits == 11.0f64.to_bits()));
        assert_eq!(site.read_leaf(p), Some(11.0));
    });
    let key = scope.root_string_ptr(key_of(b"constructor"));
    proto.with_mut_ptr::<ObjectHeader, _>(|p| {
        key.with_const_ptr(|key| crate::object::js_object_set_field_by_name(p, key, 12.0))
    });
    receiver.with_mut_ptr::<ObjectHeader, _>(|p| unsafe {
        assert_eq!(site.read(p, b"constructor"), 12.0);
    });
}
