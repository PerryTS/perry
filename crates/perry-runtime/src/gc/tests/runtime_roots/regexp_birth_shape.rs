//! #12313: the RegExp birth shape is one ShapeId for the agent's life.
//!
//! `regex::instance` memoizes the birth ShapeId as a scalar and re-stamps it
//! on every birth. Before the fix, a synchronous full trace that found no live
//! RegExp retired that record, so the next birth re-minted the same facts
//! under a NEW id and every compiled `lastIndex` read site saw one more shape
//! per collection. The record is now an external carrier, like every other
//! agent-lifetime intrinsic birth memo.
//!
//! A record that outlives its last receiver still names heap words: its keys
//! and its identity's [[Prototype]] word. The second test moves the prototype
//! with no RegExp alive and checks that the word followed it; the shape table
//! scanner visits both kinds of word (`shapes::scan_shape_table_rekey_mut`).
use super::*;
use crate::object::ObjectHeader;
use crate::string::StringHeader;

fn heap_string(bytes: &[u8]) -> *mut StringHeader {
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// A fresh `/x/g`, left unrooted: it is dead at the next collection.
fn birth() -> *mut ObjectHeader {
    crate::regex::js_regexp_new(heap_string(b"x"), heap_string(b"g"))
}

/// A minor first, so a dead nursery receiver is gone before the full trace
/// takes its carrier census.
fn collect_synchronous_full_trace() {
    gc_collect_minor();
    let _ = crate::gc::gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(
        GcTriggerKind::Direct,
    ));
}

/// An uncarried semantic shape: the control that a trace retires records.
fn unrooted_semantic_shape() -> u32 {
    let obj = crate::object::js_object_alloc(0, 0);
    unsafe { crate::object::shapes::transition_object_shape_semantics(obj) }
}

#[test]
fn regexp_birth_shape_survives_full_traces_with_no_live_regexp() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();

    let first = unsafe { crate::object::shapes::object_shape_stamp(birth()) };
    assert_ne!(first, 0);
    for round in 0..4 {
        // The control: this full trace retires uncarried records, so a birth
        // record that survives it survives because it is carried.
        let control = unrooted_semantic_shape();
        assert!(crate::object::shapes::shape_descriptor_by_id(control).is_some());
        collect_synchronous_full_trace();
        assert!(
            crate::object::shapes::shape_descriptor_by_id(control).is_none(),
            "round {round}: the full trace did not retire an uncarried shape, so it proves nothing"
        );
        assert!(
            crate::object::shapes::shape_descriptor_by_id(first).is_some(),
            "#12313 round {round}: the RegExp birth record was retired with no live RegExp"
        );
        let next = unsafe { crate::object::shapes::object_shape_stamp(birth()) };
        assert_eq!(
            next, first,
            "#12313 round {round}: a RegExp born after a full trace got a new ShapeId"
        );
    }
}

#[test]
fn regexp_birth_shape_prototype_word_follows_evacuation_with_no_live_regexp() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();

    let shape = unsafe { crate::object::shapes::object_shape_stamp(birth()) };
    let mut prototypes = vec![crate::regex::intrinsic_prototype().to_bits()];
    for round in 0..4 {
        // No RegExp is alive across these collections: only the record and
        // its memo remember the shape.
        collect_synchronous_full_trace();
        let prototype = crate::regex::intrinsic_prototype().to_bits();
        if prototypes.last() != Some(&prototype) {
            prototypes.push(prototype);
        }
        assert_eq!(
            crate::object::shapes::shape_prototype_word(shape),
            prototype,
            "#12313 round {round}: the birth record's prototype word was not repaired"
        );
        let scope = RuntimeHandleScope::new();
        let re = scope.root_raw_mut_ptr(birth());
        assert_eq!(
            re.with_mut_ptr::<ObjectHeader, _>(|r| unsafe {
                crate::object::shapes::object_shape_stamp(r)
            }),
            shape,
            "#12313 round {round}: the birth ShapeId changed"
        );
        // An inherited read walks the shape's prototype word.
        let key = scope.root_string_ptr(heap_string(b"exec"));
        let exec = re.with_mut_ptr::<ObjectHeader, _>(|r| {
            key.with_const_ptr(|k| crate::object::js_object_get_field_by_name(r, k))
        });
        assert!(
            exec.is_pointer(),
            "#12313 round {round}: RegExp.prototype.exec did not resolve through the birth shape"
        );
    }
    assert!(
        prototypes.len() > 1,
        "the intrinsic RegExp prototype never moved, so the word was never tested"
    );
}
