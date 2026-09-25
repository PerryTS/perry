//! #10498: the class-accessor cache records only what the generic path did,
//! serves it only while the recording still holds, and never serves a
//! receiver the recording does not describe.
//!
//! Every positive test asserts the counters as well as the value: a cache
//! that declines and lets the generic path answer returns the right value and
//! is invisible in the output. Every invalidation test asserts the hit
//! DECLINES — an entry that keeps matching after the mutation calls the wrong
//! accessor, which is a wrong answer, not a slow one.

use super::*;

const CAC_CLASS_ID: u32 = 0x0011_0498;
const ACCESSOR: &str = "cacLevel";

thread_local! {
    // Per test thread, so no sibling test can observe or clobber them.
    static GETTER_SAW: std::cell::Cell<(u64, u32)> = const { std::cell::Cell::new((0, 0)) };
    static SETTER_SAW: std::cell::Cell<(u64, f64, u32)> = const { std::cell::Cell::new((0, 0.0, 0)) };
}

extern "C" fn cac_getter_10498(this: f64) -> f64 {
    GETTER_SAW.with(|c| c.set((this.to_bits(), c.get().1 + 1)));
    42.0
}

extern "C" fn cac_setter_10498(this: f64, value: f64) -> f64 {
    SETTER_SAW.with(|c| c.set((this.to_bits(), value, c.get().2 + 1)));
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn getter_calls() -> u32 {
    GETTER_SAW.with(|c| c.get().1)
}

fn setter_saw() -> (u64, f64, u32) {
    SETTER_SAW.with(|c| c.get())
}

struct Scope {
    _suppress: crate::gc::GcSuppressScope,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl Scope {
    fn new() -> Self {
        let scope = Self {
            _lock: crate::gc::global_side_table_test_lock(),
            _suppress: crate::gc::GcSuppressScope::new(),
        };
        test_reset();
        unsafe { register_class() };
        scope
    }
}

unsafe fn register_class() {
    crate::object::js_register_class_id(CAC_CLASS_ID);
    crate::object::class_registry::js_register_class_getter(
        CAC_CLASS_ID as i64,
        ACCESSOR.as_ptr(),
        ACCESSOR.len() as i64,
        cac_getter_10498 as *const () as usize as i64,
    );
    crate::object::class_registry::js_register_class_setter(
        CAC_CLASS_ID as i64,
        ACCESSOR.as_ptr(),
        ACCESSOR.len() as i64,
        cac_setter_10498 as *const () as usize as i64,
    );
}

fn key(name: &str) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn key_value(key: *const crate::StringHeader) -> f64 {
    f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits())
}

fn boxed(obj: *mut ObjectHeader) -> f64 {
    f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits())
}

/// A class instance with one own data field, so it carries a ShapeId.
unsafe fn instance() -> *mut ObjectHeader {
    let obj = crate::object::js_object_alloc(CAC_CLASS_ID, 4);
    crate::object::js_object_set_field_by_name(obj, key("cacOwn"), 1.0);
    assert_ne!(
        shapes::object_shape_stamp(obj),
        0,
        "fixture: the receiver must carry a ShapeId, or nothing can be recorded"
    );
    obj
}

/// `obj.cacLevel` through the generic `[[Get]]`, which records the getter.
unsafe fn primed_getter(obj: *mut ObjectHeader, k: *const crate::StringHeader) {
    let value = crate::object::js_object_get_field_by_name(obj, k);
    assert_eq!(f64::from_bits(value.bits()), 42.0);
    assert_eq!(
        test_stats().1,
        1,
        "the generic path must have recorded the getter it called"
    );
    assert!(test_lookup(obj, k, false).is_some());
}

/// `obj.cacLevel = value` through PutValue, which records the setter.
unsafe fn put(obj: *mut ObjectHeader, k: *const crate::StringHeader, value: f64) -> f64 {
    crate::proxy::js_put_value_set(boxed(obj), key_value(k), value, boxed(obj), 0)
}

#[test]
fn a_recorded_getter_serves_the_next_read_with_the_receiver_as_this() {
    let _scope = Scope::new();
    unsafe {
        let obj = instance();
        let k = key(ACCESSOR);
        assert!(
            class_getter_hit(obj, k).is_none(),
            "nothing is recorded yet"
        );
        primed_getter(obj, k);
        let calls_before = getter_calls();
        for _ in 0..3 {
            assert_eq!(class_getter_hit(obj, k), Some(42.0));
        }
        assert_eq!(test_stats().0, 3, "every read must have been a hit");
        assert_eq!(
            getter_calls(),
            calls_before + 3,
            "each hit calls the getter"
        );
        assert_eq!(GETTER_SAW.with(|c| c.get().0), boxed(obj).to_bits());
    }
}

/// An entry is keyed on the SHAPE, so a second receiver built the same way is
/// served by the first one's recording. Both key-adds run back to back so the
/// second reuses the first's transition edge (see the inherited-read cache's
/// test of the same claim for why the order is the test).
#[test]
fn a_second_receiver_of_the_same_shape_is_served_by_one_entry() {
    let _scope = Scope::new();
    unsafe {
        let first = crate::object::js_object_alloc(CAC_CLASS_ID, 4);
        let second = crate::object::js_object_alloc(CAC_CLASS_ID, 4);
        let own = key("cacShared");
        crate::object::js_object_set_field_by_name(first, own, 1.0);
        crate::object::js_object_set_field_by_name(second, own, 2.0);
        assert_eq!(
            shapes::object_shape_stamp(first),
            shapes::object_shape_stamp(second),
            "fixture: both receivers must carry one ShapeId"
        );
        let k = key(ACCESSOR);
        primed_getter(first, k);
        assert_eq!(class_getter_hit(second, k), Some(42.0));
        assert_eq!(GETTER_SAW.with(|c| c.get().0), boxed(second).to_bits());
    }
}

#[test]
fn a_setter_is_recorded_by_the_set_walk_and_serves_putvalue() {
    let _scope = Scope::new();
    unsafe {
        let obj = instance();
        let k = key(ACCESSOR);
        assert_eq!(put(obj, k, 6.0), 6.0);
        assert_eq!(setter_saw().1, 6.0, "the generic walk must call the setter");
        assert_eq!(test_stats(), (0, 1), "and record it");

        let calls = setter_saw().2;
        assert_eq!(class_setter_hit(obj, k, 7.0), Some(7.0));
        assert_eq!(setter_saw(), (boxed(obj).to_bits(), 7.0, calls + 1));
        // PutValue itself now takes the hit, strict or sloppy.
        assert_eq!(
            crate::proxy::js_put_value_set(boxed(obj), key_value(k), 8.0, boxed(obj), 1),
            8.0
        );
        assert_eq!(setter_saw(), (boxed(obj).to_bits(), 8.0, calls + 2));
        assert_eq!(test_stats().0, 2);
    }
}

/// `js_object_set_field_by_name` reaches the same vtable arm from callers
/// whose semantics are not `[[Set]]`'s, so a direct call must not record.
#[test]
fn a_direct_named_store_calls_the_setter_but_records_nothing() {
    let _scope = Scope::new();
    unsafe {
        let obj = instance();
        let k = key(ACCESSOR);
        crate::object::js_object_set_field_by_name(obj, k, 5.0);
        assert_eq!(setter_saw().1, 5.0);
        assert_eq!(test_stats().1, 0);
        assert!(test_lookup(obj, k, true).is_none());
    }
}

/// A write whose target is NOT the receiver resolves over the target's chain,
/// so it says nothing about the receiver's own `[[Set]]`.
#[test]
fn a_set_with_a_different_target_records_nothing() {
    let _scope = Scope::new();
    unsafe {
        let obj = instance();
        let other = crate::object::js_object_alloc(0, 2);
        let k = key(ACCESSOR);
        crate::proxy::js_put_value_set(boxed(other), key_value(k), 3.0, boxed(obj), 0);
        assert_eq!(test_stats().1, 0);
    }
}

#[test]
fn the_setter_probe_matches_only_its_receiver_and_key_and_only_once() {
    let _scope = Scope::new();
    unsafe {
        let obj = instance();
        let k = key(ACCESSOR);
        let wrong = key("cacOther");
        let setter = cac_setter_10498 as *const () as usize;
        let prev = arm_setter_probe(boxed(obj), key_value(k));
        note_class_setter(obj, wrong, setter);
        assert_eq!(
            SETTER_PROBE.with(|p| p.get()).seen,
            None,
            "another key must not match"
        );
        note_class_setter(obj, k, setter);
        let seen = SETTER_PROBE.with(|p| p.get()).seen;
        assert!(seen.is_some(), "the armed receiver and key must match");
        assert_eq!(
            test_stats().1,
            0,
            "the vtable arm captures; it never commits"
        );
        // A nested store inside the setter body cannot match the same probe.
        note_class_setter(obj, k, 0x1234);
        assert_eq!(SETTER_PROBE.with(|p| p.get()).seen, seen);
        finish_setter_probe(prev, key_value(k));
        assert_eq!(test_stats().1, 1, "the arming frame commits on return");
        assert_eq!(test_lookup(obj, k, true), Some(setter));
        assert_eq!(
            SETTER_PROBE.with(|p| p.get()),
            prev,
            "the displaced probe is back"
        );
    }
}

/// A throw out of `target_set` skips the arming frame's commit; the probe it
/// leaves behind must never be committed by anyone else.
#[test]
fn an_abandoned_probe_is_never_committed() {
    let _scope = Scope::new();
    unsafe {
        let obj = instance();
        let k = key(ACCESSOR);
        let setter = cac_setter_10498 as *const () as usize;
        // The abandoned frame: armed, matched, never finished.
        let _abandoned_prev = arm_setter_probe(boxed(obj), key_value(k));
        note_class_setter(obj, k, setter);
        // A later walk arms over it and finishes having seen nothing.
        let prev = arm_setter_probe(boxed(obj), key_value(key("cacOther")));
        finish_setter_probe(prev, key_value(key("cacOther")));
        assert_eq!(test_stats().1, 0);
        assert!(test_lookup(obj, k, true).is_none());
        // A direct store after it matches nothing either: the abandoned
        // probe was disarmed by its own match.
        crate::object::js_object_set_field_by_name(obj, k, 4.0);
        assert!(test_lookup(obj, k, true).is_none());
    }
}

#[test]
fn a_shape_transition_on_the_receiver_retires_its_entry() {
    let _scope = Scope::new();
    unsafe {
        let obj = instance();
        let k = key(ACCESSOR);
        primed_getter(obj, k);
        crate::object::js_object_set_field_by_name(obj, key("cacLater"), 3.0);
        assert!(class_getter_hit(obj, k).is_none());
    }
}

/// An own data property named like the accessor shadows it.
#[test]
fn an_own_property_shadowing_the_accessor_is_never_served() {
    let _scope = Scope::new();
    unsafe {
        let obj = instance();
        let k = key(ACCESSOR);
        primed_getter(obj, k);
        assert!(crate::proxy::create_data_property(
            boxed(obj),
            key_value(k),
            9.0
        ));
        assert!(class_getter_hit(obj, k).is_none());
        assert!(class_setter_hit(obj, k, 1.0).is_none());
        let value = crate::object::js_object_get_field_by_name(obj, k);
        assert_eq!(f64::from_bits(value.bits()), 9.0);
    }
}

#[test]
fn a_semantic_property_event_anywhere_retires_every_entry() {
    let _scope = Scope::new();
    unsafe {
        let obj = instance();
        let k = key(ACCESSOR);
        primed_getter(obj, k);
        crate::object::prop_plan::prop_plan_epoch_bump();
        assert!(class_getter_hit(obj, k).is_none());
    }
}

#[test]
fn a_vtable_registration_retires_every_entry() {
    let _scope = Scope::new();
    unsafe {
        let obj = instance();
        let k = key(ACCESSOR);
        primed_getter(obj, k);
        crate::object::class_registry::test_bump_vtable_generation();
        assert!(class_getter_hit(obj, k).is_none());
    }
}

/// `Object.setPrototypeOf(instance, other)` gives the receiver a chain of its
/// own: its recorded prototype refuses the hit even before the epoch does.
#[test]
fn a_receiver_with_its_own_prototype_is_refused() {
    let _scope = Scope::new();
    unsafe {
        let obj = instance();
        let k = key(ACCESSOR);
        primed_getter(obj, k);
        let proto = crate::object::js_object_alloc(0, 2);
        crate::object::js_object_set_prototype_of(boxed(obj), boxed(proto));
        assert!(class_getter_hit(obj, k).is_none());
    }
}

#[test]
fn a_non_extensible_receiver_is_refused_for_writes() {
    let _scope = Scope::new();
    unsafe {
        let obj = instance();
        let k = key(ACCESSOR);
        put(obj, k, 6.0);
        assert!(test_lookup(obj, k, true).is_some());
        crate::object::js_object_prevent_extensions(boxed(obj));
        assert!(class_setter_hit(obj, k, 1.0).is_none());
    }
}

/// An inherited walk's armed receiver would be the getter's `this`: never
/// recorded under one, never served under one.
#[test]
fn an_armed_accessor_receiver_blocks_the_hit() {
    let _scope = Scope::new();
    unsafe {
        let obj = instance();
        let k = key(ACCESSOR);
        primed_getter(obj, k);
        let prev = crate::object::field_get_set::accessor_receiver_override_begin(boxed(obj));
        assert!(class_getter_hit(obj, k).is_none());
        crate::object::field_get_set::accessor_receiver_override_end(prev);
        assert_eq!(class_getter_hit(obj, k), Some(42.0));
    }
}

#[test]
fn a_dead_key_is_pruned() {
    let _scope = Scope::new();
    unsafe {
        let obj = instance();
        let k = key(ACCESSOR);
        primed_getter(obj, k);
        prune_dead_class_accessor_cache_entries(&|addr| addr == k as usize);
        assert!(test_lookup(obj, k, false).is_none());
    }
}

#[test]
fn keys_answered_by_special_arms_are_never_cacheable() {
    for bytes in [
        &b"#priv"[..],
        b"0",
        b"12",
        b"-1",
        b"length",
        b"size",
        b"constructor",
        b"__proto__",
        b"prototype",
        b"name",
        b"then",
        b"toJSON",
    ] {
        assert!(
            !key_bytes_cacheable(bytes),
            "{:?}",
            std::str::from_utf8(bytes)
        );
    }
    for bytes in [&b"points"[..], b"ms", b"remainingPoints", b"", b"x1"] {
        assert!(
            key_bytes_cacheable(bytes),
            "{:?}",
            std::str::from_utf8(bytes)
        );
    }
}

/// The setter hit skips its root when the assigned value cannot name the
/// heap. Everything that can must stay rooted, or PutValue answers a
/// from-space address after a setter that collects.
#[test]
fn only_values_that_can_name_the_heap_skip_the_root() {
    for immediate in [
        1.5f64.to_bits(),
        (-0.0f64).to_bits(),
        f64::NAN.to_bits(),
        f64::NEG_INFINITY.to_bits(),
        crate::value::TAG_UNDEFINED,
        crate::value::TAG_NULL,
        crate::value::TAG_TRUE,
        crate::value::TAG_HOLE,
        crate::value::INT32_TAG | 7,
        crate::value::SHORT_STRING_TAG | 0x61,
    ] {
        assert!(!nanbox_may_name_heap(immediate), "{immediate:#x}");
    }
    for heap in [
        crate::value::POINTER_TAG | 0x1_0000_0000,
        crate::value::STRING_TAG | 0x1_0000_0000,
        crate::value::BIGINT_TAG | 0x1_0000_0000,
        crate::value::JS_HANDLE_TAG | 1,
        0x1_0000_0000,
    ] {
        assert!(nanbox_may_name_heap(heap), "{heap:#x}");
    }
}

#[test]
fn a_setter_hit_answers_a_heap_value_through_its_root() {
    let _scope = Scope::new();
    unsafe {
        let obj = instance();
        let k = key(ACCESSOR);
        put(obj, k, 1.0);
        let payload = key_value(key("cacPayloadString"));
        assert_eq!(
            class_setter_hit(obj, k, payload).map(f64::to_bits),
            Some(payload.to_bits())
        );
        assert_eq!(setter_saw().1.to_bits(), payload.to_bits());
    }
}
