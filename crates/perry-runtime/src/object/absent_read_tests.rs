//! Invalidation suite for the absent verdict (#10495).
//!
//! An absent entry is the one entry in the inherited-read table that RETURNS A
//! VALUE the runtime never looked up. Every test here therefore asserts two
//! things the program's output cannot show on its own: that the verdict was
//! actually recorded (a mechanism that silently declines is correct and
//! useless, and looks identical from outside), and that after a mutation it is
//! NOT served (an entry that keeps matching answers `undefined` for a key that
//! is present — a silent wrong value, which is what this campaign has produced
//! six of).
//!
//! Every test in this file is red on a tree built with the guard it exercises
//! removed; see the PR for the sabotage build and its output.

use super::*;
use crate::object::inherited_read_cache::{
    inherited_read_cache_lookup, inherited_read_cache_prime_with_absence, test_clear_cache, Lookup,
};
use crate::object::ObjectHeader;

fn key(name: &str) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn set(obj: *mut ObjectHeader, name: &str, value: f64) {
    crate::object::js_object_set_field_by_name(obj, key(name), value);
}

fn set_undefined(obj: *mut ObjectHeader, name: &str) {
    crate::object::js_object_set_field_by_name(
        obj,
        key(name),
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
}

fn boxed(obj: *mut ObjectHeader) -> f64 {
    f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits())
}

/// These tests assert entry IDENTITY, so a collection moving an object
/// mid-test would make an invalidation assertion pass for the wrong reason.
struct AbsentScope {
    _suppress: crate::gc::GcSuppressScope,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl AbsentScope {
    fn new() -> Self {
        let lock = crate::gc::global_side_table_test_lock();
        let scope = Self {
            _suppress: crate::gc::GcSuppressScope::new(),
            _lock: lock,
        };
        test_clear_cache();
        crate::object::inherited_read_cache::test_reset_counters();
        test_reset_absent_counters();
        scope
    }
}

/// The production path for one read that reaches the miss handler's tail:
/// ask the walk, then run the tail and record only if BOTH halves hold.
/// Tests drive this rather than `record_absent` directly, so what they
/// exercise is what `ic_miss.rs` calls.
unsafe fn read_through_miss_tail(obj: *mut ObjectHeader, k: *const crate::StringHeader) -> f64 {
    match inherited_read_cache_lookup(obj, k) {
        Lookup::Hit(v) => return f64::from_bits(v.bits()),
        Lookup::Absent => return f64::from_bits(crate::value::TAG_UNDEFINED),
        Lookup::Declined => {
            return f64::from_bits(tail_and_maybe_record_absent(obj, k, None).bits());
        }
        Lookup::Unknown => {}
    }
    let (value, exhausted) = inherited_read_cache_prime_with_absence(obj, k);
    if let Some(value) = value {
        return f64::from_bits(value.bits());
    }
    f64::from_bits(tail_and_maybe_record_absent(obj, k, exhausted).bits())
}

unsafe fn is_absent_entry(obj: *mut ObjectHeader, k: *const crate::StringHeader) -> bool {
    matches!(inherited_read_cache_lookup(obj, k), Lookup::Absent)
}

/// `O -> P -> Object.prototype`, with `P` an ordinary literal-shaped object.
unsafe fn one_level() -> (*mut ObjectHeader, *mut ObjectHeader) {
    let proto = crate::object::js_object_alloc(0, 4);
    set(proto, "abs_present", 7.0);
    let obj = crate::object::js_object_alloc(0, 4);
    crate::object::js_object_set_prototype_of(boxed(obj), boxed(proto));
    (obj, proto)
}

// --- the mechanism ----------------------------------------------------------

#[test]
fn an_absent_key_is_recorded_once_and_then_answered_without_walking() {
    let _scope = AbsentScope::new();
    unsafe {
        let (obj, _proto) = one_level();
        let k = key("abs_missing");
        assert!(!is_absent_entry(obj, k), "nothing is recorded yet");

        let first = read_through_miss_tail(obj, k);
        assert_eq!(first.to_bits(), crate::value::TAG_UNDEFINED);
        assert_eq!(
            absent_recorded(),
            1,
            "the first read must record the verdict"
        );

        assert!(
            is_absent_entry(obj, k),
            "the second read must be answered from the table"
        );
        assert_eq!(
            absent_recorded(),
            1,
            "a served verdict must not re-walk and re-record"
        );
    }
}

/// THE regression this mechanism exists to not have.
///
/// `P.a = undefined` puts `a` in `P`'s key LIST holding `undefined`. The
/// generic tail answers `undefined`, which is byte-identical to what it
/// answers for a key that is on nothing — so an "observed `undefined`" rule
/// alone records an absent verdict here. It is wrong: `"a" in o` is `true`,
/// and `P.a = 5` is a plain VALUE store to an EXISTING key, which transitions
/// no shape, installs no descriptor, touches no registry and moves not one
/// guard. The entry would answer `undefined` for the life of the process.
///
/// The walk is what tells the two apart, because it looks at the key list
/// rather than at the value, and `record_absent` requires the walk's token as
/// well as the observation.
#[test]
fn a_prototype_key_holding_undefined_is_not_absent() {
    let _scope = AbsentScope::new();
    unsafe {
        let proto = crate::object::js_object_alloc(0, 4);
        set_undefined(proto, "abs_u");
        let obj = crate::object::js_object_alloc(0, 4);
        crate::object::js_object_set_prototype_of(boxed(obj), boxed(proto));
        let k = key("abs_u");

        let first = read_through_miss_tail(obj, k);
        assert_eq!(
            first.to_bits(),
            crate::value::TAG_UNDEFINED,
            "the read itself is undefined, which is exactly the trap"
        );
        assert_eq!(
            absent_recorded(),
            0,
            "a key that IS in a hop's key list must never be recorded absent"
        );
        assert!(!is_absent_entry(obj, k));

        // The store a wrong entry would have made invisible.
        set(proto, "abs_u", 5.0);
        let after = read_through_miss_tail(obj, k);
        assert_eq!(
            after, 5.0,
            "a plain value store to an existing prototype key must be observed"
        );
    }
}

#[test]
fn the_observation_half_cannot_be_asserted_without_an_undefined() {
    assert!(ObservedUndefined::from_tail_answer(JSValue::undefined()).is_some());
    assert!(ObservedUndefined::from_tail_answer(JSValue::null()).is_none());
    assert!(ObservedUndefined::from_tail_answer(JSValue::number(0.0)).is_none());
}

// --- invalidation -----------------------------------------------------------

#[test]
fn a_key_added_to_the_prototype_reopens_an_absent_pair() {
    let _scope = AbsentScope::new();
    unsafe {
        let (obj, proto) = one_level();
        let k = key("abs_later");
        assert_eq!(
            read_through_miss_tail(obj, k).to_bits(),
            crate::value::TAG_UNDEFINED
        );
        assert_eq!(absent_recorded(), 1);
        assert!(is_absent_entry(obj, k));

        set(proto, "abs_later", 9.0);
        assert!(
            !is_absent_entry(obj, k),
            "a key added to a hop is a shape transition on that hop"
        );
        assert_eq!(read_through_miss_tail(obj, k), 9.0);
    }
}

#[test]
fn a_key_added_to_the_receiver_shadows_an_absent_pair() {
    let _scope = AbsentScope::new();
    unsafe {
        let (obj, _proto) = one_level();
        let k = key("abs_own");
        assert_eq!(
            read_through_miss_tail(obj, k).to_bits(),
            crate::value::TAG_UNDEFINED
        );
        assert!(is_absent_entry(obj, k));

        set(obj, "abs_own", 3.0);
        assert!(
            !is_absent_entry(obj, k),
            "the receiver's own key add mints a new ShapeId"
        );
        assert_eq!(read_through_miss_tail(obj, k), 3.0);
    }
}

#[test]
fn set_prototype_of_reopens_an_absent_pair() {
    let _scope = AbsentScope::new();
    unsafe {
        let (obj, _proto) = one_level();
        let k = key("abs_swap");
        assert_eq!(
            read_through_miss_tail(obj, k).to_bits(),
            crate::value::TAG_UNDEFINED
        );
        assert!(is_absent_entry(obj, k));

        let other = crate::object::js_object_alloc(0, 4);
        set(other, "abs_swap", 11.0);
        crate::object::js_object_set_prototype_of(boxed(obj), boxed(other));
        assert!(!is_absent_entry(obj, k));
        assert_eq!(read_through_miss_tail(obj, k), 11.0);
    }
}

#[test]
fn an_accessor_on_the_prototype_is_declined_rather_than_recorded_absent() {
    let _scope = AbsentScope::new();
    unsafe {
        let (obj, proto) = one_level();
        crate::object::descriptor_state::set_accessor_descriptor(
            proto as usize,
            "abs_acc".to_string(),
            crate::object::descriptor_state::AccessorDescriptor { get: 0, set: 0 },
        );
        let k = key("abs_acc");
        read_through_miss_tail(obj, k);
        assert_eq!(
            absent_recorded(),
            0,
            "a hop the walk refuses leaves objects unexamined; nothing may be claimed"
        );
        assert!(!is_absent_entry(obj, k));
    }
}

/// `vtable_generation()` is the guard the other two words do not cover:
/// registering a prototype method is a side-table write that transitions no
/// shape and installs no descriptor.
#[test]
fn a_vtable_registration_invalidates_an_absent_pair() {
    let _scope = AbsentScope::new();
    unsafe {
        let (obj, _proto) = one_level();
        let k = key("abs_vt");
        assert_eq!(
            read_through_miss_tail(obj, k).to_bits(),
            crate::value::TAG_UNDEFINED
        );
        assert!(is_absent_entry(obj, k));

        crate::object::class_registry::test_bump_vtable_generation();
        assert!(
            !is_absent_entry(obj, k),
            "a method/getter/setter registration must reopen every absent verdict"
        );
    }
}

/// The four class-registry writes `VTABLE_GEN` deliberately does not bump.
#[test]
fn a_class_lookup_surface_write_invalidates_an_absent_pair() {
    let _scope = AbsentScope::new();
    unsafe {
        let (obj, _proto) = one_level();
        let k = key("abs_surface");
        assert_eq!(
            read_through_miss_tail(obj, k).to_bits(),
            crate::value::TAG_UNDEFINED
        );
        assert!(is_absent_entry(obj, k));

        crate::object::class_registry::class_lookup_surface_gen_bump();
        assert!(!is_absent_entry(obj, k));
    }
}

/// A descriptor install anywhere bumps the semantic epoch, which the entry
/// already carries; this pins that the absent entry honours it too.
#[test]
fn a_descriptor_install_invalidates_an_absent_pair() {
    let _scope = AbsentScope::new();
    unsafe {
        let (obj, proto) = one_level();
        let k = key("abs_desc");
        assert_eq!(
            read_through_miss_tail(obj, k).to_bits(),
            crate::value::TAG_UNDEFINED
        );
        assert!(is_absent_entry(obj, k));

        crate::object::descriptor_state::set_accessor_descriptor(
            proto as usize,
            "abs_unrelated".to_string(),
            crate::object::descriptor_state::AccessorDescriptor { get: 0, set: 0 },
        );
        assert!(!is_absent_entry(obj, k));
    }
}
