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
        test_reset_walk_stops();
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

/// #10842 marks a hop the FIRST time any walk traverses it and abandons that
/// walk, so the first read of a pair through a not-yet-marked prototype
/// records nothing — and `Object.prototype` is a singleton, so WHICH test
/// pays that depends on test order. Every test therefore reads until a read
/// completes without an abandon and asserts on that read. Bounded by one
/// abandon per hop plus one full walk.
unsafe fn read_settled(obj: *mut ObjectHeader, k: *const crate::StringHeader) -> f64 {
    for _ in 0..6 {
        let before = test_walk_stop_count(walk_stop::HOP_UNMARKED);
        let value = read_through_miss_tail(obj, k);
        if test_walk_stop_count(walk_stop::HOP_UNMARKED) == before {
            return value;
        }
    }
    panic!("the walk kept abandoning on unmarked hops");
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

        let first = read_settled(obj, k);
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

        let first = read_settled(obj, k);
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
        assert_eq!(
            test_last_walk_stop(),
            walk_stop::FOUND_UNDEFINED,
            "and the walk must have refused because it SAW the key, not because it abandoned"
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
        assert_eq!(read_settled(obj, k).to_bits(), crate::value::TAG_UNDEFINED);
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
        assert_eq!(read_settled(obj, k).to_bits(), crate::value::TAG_UNDEFINED);
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
        assert_eq!(read_settled(obj, k).to_bits(), crate::value::TAG_UNDEFINED);
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
        read_settled(obj, k);
        assert_eq!(
            absent_recorded(),
            0,
            "a hop the walk refuses leaves objects unexamined; nothing may be claimed"
        );
        assert_eq!(
            test_last_walk_stop(),
            walk_stop::HOP_BLOOM,
            "and it must be the accessor Bloom bit on the hop that refused"
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
        assert_eq!(read_settled(obj, k).to_bits(), crate::value::TAG_UNDEFINED);
        assert!(is_absent_entry(obj, k));

        crate::object::class_registry::test_bump_vtable_generation();
        assert!(
            !is_absent_entry(obj, k),
            "a method/getter/setter registration must reopen every absent verdict"
        );
    }
}

/// The four class-registry writes `VTABLE_GEN` deliberately does not bump.
/// #10842 routed `class_lookup_surface_gen_bump` into `proto_validity`, which
/// the entry already carries; this pins that the fold covers the absent entry
/// too, so it cannot be undone without this going red.
#[test]
fn a_class_lookup_surface_write_invalidates_an_absent_pair() {
    let _scope = AbsentScope::new();
    unsafe {
        let (obj, _proto) = one_level();
        let k = key("abs_surface");
        assert_eq!(read_settled(obj, k).to_bits(), crate::value::TAG_UNDEFINED);
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
        assert_eq!(read_settled(obj, k).to_bits(), crate::value::TAG_UNDEFINED);
        assert!(is_absent_entry(obj, k));

        crate::object::descriptor_state::set_accessor_descriptor(
            proto as usize,
            "abs_unrelated".to_string(),
            crate::object::descriptor_state::AccessorDescriptor { get: 0, set: 0 },
        );
        assert!(!is_absent_entry(obj, k));
    }
}

/// `fx/inval.ts` route 6 — the silent wrong value the differential fixture
/// found while every test above was green (design doc §20.1). Delete an own
/// key, warm the miss, re-add the key. The O(1) delete tombstones the
/// keys-array slot IN PLACE with `TAG_HOLE` (`delete_rest.rs`), the read then
/// reports not-own, and the re-add refills that slot under an UNCHANGED
/// ShapeId — so a verdict recorded in between answers `undefined` for a key
/// that is present. Compiled program before the scan: `6-readded undefined`;
/// node: `12`.
///
/// Enforcement point s3, `receiver_key_list_admits_absent`: a receiver whose
/// key list holds this key OR a tombstone is never recorded. The test pins
/// that the delete really tombstoned (`hole_count`), so it cannot pass by
/// taking a different delete lane, and that the key-list scan is the arm that
/// refused, so it cannot pass on an abandon. Red with the scan removed.
#[test]
fn a_deleted_then_re_added_own_key_is_never_answered_absent() {
    let _scope = AbsentScope::new();
    let _tombstones = crate::object::delete_rest::test_scope_tombstone_deletes(true);
    unsafe {
        let obj = crate::object::js_object_alloc(0, 4);
        set(obj, "abs_x", 0.0);
        set(obj, "abs_gone", 11.0);
        let k = key("abs_gone");
        assert_eq!(crate::object::js_object_delete_field(obj, k), 1);
        let shape = crate::object::shapes::object_shape_descriptor(obj).expect("a shaped receiver");
        assert!(
            shape.hole_count >= 1,
            "the delete must tombstone in place, or this test exercises nothing"
        );

        for _ in 0..3 {
            assert_eq!(read_settled(obj, k).to_bits(), crate::value::TAG_UNDEFINED);
        }
        assert_eq!(
            absent_recorded(),
            0,
            "a receiver whose key list holds a tombstone must never be recorded absent"
        );
        assert_eq!(
            test_last_walk_stop(),
            walk_stop::RECORD_RECV_KEY_LIST,
            "and it must be the key-list scan that refused"
        );
        assert!(!is_absent_entry(obj, k));

        set(obj, "abs_gone", 12.0);
        assert_eq!(
            read_through_miss_tail(obj, k),
            12.0,
            "route 6 of fx/inval.ts: node prints 12"
        );
    }
}

/// The chain end that is not an object. A fresh thread has no realm global,
/// hence no `Object.prototype`, hence a memo of 0 — the state every compiled
/// program starts in, and the one refusal (`implicit_no_memo`) that EVERY
/// declining chain on the miss fixtures left the walk by. The verdict is
/// recordable there, and it must die the moment the realm is materialized:
/// that allocates an `Object.prototype` nothing has marked, and a key added
/// to it afterwards bumps no validity word.
///
/// Enforcement point s4, `AbsentGuards::realm_cold`. Red with the bit
/// ignored: the entry outlives the realm and `abs_cold` on `Object.prototype`
/// is never seen. Runs on its own thread because the realm, the memo and the
/// cache are all thread-local, and the test process's main thread has long
/// since materialized its realm.
#[test]
fn a_verdict_recorded_before_the_realm_exists_dies_with_its_materialization() {
    let _lock = crate::gc::global_side_table_test_lock();
    std::thread::spawn(|| unsafe {
        let _suppress = crate::gc::GcSuppressScope::new();
        test_clear_cache();
        crate::object::inherited_read_cache::test_reset_counters();
        test_reset_absent_counters();
        test_reset_walk_stops();
        assert!(
            !crate::object::global_this_is_materialized(),
            "premise: a fresh thread has no realm global"
        );
        let obj = crate::object::js_object_alloc(0, 4);
        set(obj, "abs_x", 0.0);
        let k = key("abs_cold");

        assert_eq!(read_settled(obj, k).to_bits(), crate::value::TAG_UNDEFINED);
        assert_eq!(
            absent_recorded(),
            1,
            "a realm that does not exist is a proved chain end"
        );
        assert!(
            test_walk_stop_count(walk_stop::END_COLD_REALM) >= 1,
            "and it must be THAT end the walk reached"
        );
        assert!(is_absent_entry(obj, k));

        // Materialize the realm: `Object.prototype` now exists, unmarked.
        let _realm = crate::object::js_get_global_this();
        assert!(crate::object::global_this_is_materialized());
        assert!(
            !is_absent_entry(obj, k),
            "an entry recorded on a cold realm must not outlive its materialization"
        );

        // Re-proved on the real chain end (#10842 marks Object.prototype on
        // the first walk through it), then invalidated by a key on it.
        assert_eq!(read_settled(obj, k).to_bits(), crate::value::TAG_UNDEFINED);
        assert_eq!(absent_recorded(), 2);
        assert!(test_walk_stop_count(walk_stop::END_OBJECT_PROTOTYPE) >= 1);
        let object_prototype = crate::array::object_prototype_addr() as *mut ObjectHeader;
        assert!(
            !object_prototype.is_null(),
            "the realm resolves Object.prototype"
        );
        set(object_prototype, "abs_cold", 21.0);
        assert!(
            !is_absent_entry(obj, k),
            "a key added to the marked Object.prototype bumps the validity word"
        );
        assert_eq!(read_through_miss_tail(obj, k), 21.0);
    })
    .join()
    .expect("the cold-realm thread must not panic");
}
