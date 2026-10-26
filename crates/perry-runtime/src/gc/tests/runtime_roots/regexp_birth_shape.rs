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

use crate::codegen_abi::LayoutWord;
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
    unsafe { crate::object::shapes::transition_object_shape_semantics(obj).0 }
}

#[test]
fn regexp_birth_shape_survives_full_traces_with_no_live_regexp() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();

    let first = unsafe { crate::object::shapes::object_shape_stamp(birth()) };
    assert_ne!(first, LayoutWord(0));
    for round in 0..4 {
        // The control: this full trace retires uncarried records, so a birth
        // record that survives it survives because it is carried.
        let control = unrooted_semantic_shape();
        assert!(
            crate::object::shapes::test_descriptor_for_retained_word(LayoutWord(control)).is_some()
        );
        collect_synchronous_full_trace();
        assert!(
            crate::object::shapes::shape_is_retired(LayoutWord(control)),
            "round {round}: the full trace did not retire an uncarried shape, so it proves nothing"
        );
        assert!(
            crate::object::shapes::test_descriptor_for_retained_word(first).is_some(),
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
    // The intrinsic is born old and never-move (`gc::never_move`): no
    // collection may move it, so the word stays valid by construction.
    assert_eq!(prototypes.len(), 1, "the never-move RegExp prototype moved");
}

/// A birth memo holds an agent-lifetime carrier (the intrinsic birth records
/// do); an uncarried shape retires, and its LayoutWord must then never be
/// dereferenced. The identity is what outlives the record: it reads None
/// without forming a Region address, and a birth takes publication instead.
#[test]
fn plain_birth_rejects_a_retired_shape_after_full_collection() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let object = crate::object::object_alloc_plain(0);
    let shape = unsafe { crate::object::shapes::transition_object_shape_semantics(object) };
    assert!(crate::object::shapes::plain_birth_rep_for_word(shape, 0).is_some());
    let identity = crate::object::shapes::shape_record_for_word(shape)
        .expect("live record")
        .identity();
    collect_synchronous_full_trace();
    assert!(
        crate::object::shapes::shape_is_retired(shape),
        "control shape must retire"
    );
    assert!(
        crate::object::shapes::handle_by_identity(identity).is_none(),
        "a retired identity reads absent without touching the Region"
    );
}

/// The birth remembers the intrinsic identity, not a prototype address or
/// its old property layout. Exercise edits, a replaced parent, then a full
/// trace and forced evacuation before reading a freshly born receiver.
#[test]
fn regexp_birth_observes_prototype_edits_and_replaced_parent_after_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let shape = unsafe { crate::object::shapes::object_shape_stamp(birth()) };
    let prototype = scope.root_raw_mut_ptr(crate::value::js_nanbox_get_pointer(
        crate::regex::intrinsic_prototype(),
    ) as *mut ObjectHeader);
    let prototype_value = || {
        prototype.with_const_ptr::<ObjectHeader, _>(|p| crate::value::js_nanbox_pointer(p as i64))
    };
    let original_parent =
        scope.root_nanbox_f64(crate::object::js_object_get_prototype_of(prototype_value()));
    let key = scope.root_string_ptr(heap_string(b"lit12312_probe"));
    let parent = scope.root_raw_mut_ptr(crate::object::object_alloc_plain(0));
    parent.with_mut_ptr::<ObjectHeader, _>(|p| {
        key.with_const_ptr(|k| crate::object::js_object_set_field_by_name(p, k, 17.0));
    });
    assert_eq!(
        crate::proxy::js_reflect_set_prototype_of(
            prototype_value(),
            parent.with_mut_ptr::<ObjectHeader, _>(|p| crate::value::js_nanbox_pointer(p as i64)),
        )
        .to_bits(),
        crate::value::TAG_TRUE,
    );
    let before = prototype_value().to_bits();
    for value in [17.0, 29.0] {
        if value == 29.0 {
            // A mutation of the intrinsic's own layout now shadows its parent.
            prototype.with_mut_ptr::<ObjectHeader, _>(|p| {
                key.with_const_ptr(|k| crate::object::js_object_set_field_by_name(p, k, value));
            });
        }
        collect_synchronous_full_trace();
        let receiver = scope.root_raw_mut_ptr(birth());
        assert_eq!(
            receiver.with_mut_ptr::<ObjectHeader, _>(|p| unsafe {
                crate::object::shapes::object_shape_stamp(p)
            }),
            shape
        );
        let observed = receiver.with_mut_ptr::<ObjectHeader, _>(|p| {
            key.with_const_ptr(|k| crate::object::js_object_get_field_by_name(p, k))
        });
        assert_eq!(observed.as_number(), value);
        let index = scope.root_string_ptr(heap_string(b"lastIndex"));
        assert_eq!(
            receiver
                .with_mut_ptr::<ObjectHeader, _>(|p| {
                    index.with_const_ptr(|k| crate::object::js_object_get_field_by_name(p, k))
                })
                .as_number(),
            0.0
        );
        let subject = scope.root_string_ptr(heap_string(b"x"));
        assert_eq!(
            receiver.with_mut_ptr::<ObjectHeader, _>(|p| {
                subject.with_mut_ptr(|s| crate::regex::js_regexp_test(p, s))
            }),
            1
        );
    }
    assert_eq!(
        before,
        prototype_value().to_bits(),
        "the never-move intrinsic prototype must not move"
    );
    assert_eq!(
        crate::proxy::js_reflect_set_prototype_of(
            prototype_value(),
            original_parent.get_nanbox_f64(),
        )
        .to_bits(),
        crate::value::TAG_TRUE
    );
    prototype.with_mut_ptr::<ObjectHeader, _>(|p| {
        key.with_const_ptr(|k| crate::object::js_object_delete_field(p, k));
    });
}

/// Retirement safety: a layout word still named by a runtime birth cache (here
/// the arguments cache) is a cache carrier, so a full trace with no live
/// receiver must not retire or decommit its row. Fails if a holder dereferences
/// words nothing carries.
#[test]
fn arguments_birth_cache_word_is_carried_across_full_traces() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    // Production `gc_init` registers the shape-table scanner; this window
    // needs it because it is the scanner that carries the arguments cache's
    // word.
    crate::gc::gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    crate::object::test_clear_arguments_object_roots();
    let word = {
        let scope = RuntimeHandleScope::new();
        let callee = f64::from_bits(crate::value::TAG_UNDEFINED);
        let obj = crate::object::arguments_object_alloc_rooted(&[], callee, false, 0);
        let obj = scope.root_raw_mut_ptr(obj);
        obj.with_const_ptr::<ObjectHeader, _>(|o| unsafe {
            crate::object::shapes::object_shape_stamp(o)
        })
    };
    for round in 0..3 {
        let control = unrooted_semantic_shape();
        collect_synchronous_full_trace();
        assert!(
            crate::object::shapes::shape_is_retired(LayoutWord(control)),
            "round {round}: control must retire"
        );
        assert!(
            !crate::object::shapes::shape_is_retired(word),
            "round {round}: a word held by the arguments cache was retired"
        );
    }
}

/// Same contract for the keyless-birth allocation memo: the word it names is
/// dereferenced on the next birth, so a full trace must not retire it.
#[test]
fn keyless_birth_memo_word_is_carried_across_full_traces() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let word =
        unsafe { crate::object::shapes::object_shape_stamp(crate::object::object_alloc_plain(0)) };
    for round in 0..3 {
        let control = unrooted_semantic_shape();
        collect_synchronous_full_trace();
        assert!(
            crate::object::shapes::shape_is_retired(LayoutWord(control)),
            "round {round}: control must retire"
        );
        assert!(
            !crate::object::shapes::shape_is_retired(word),
            "round {round}: the keyless birth memo word was retired"
        );
        // The next birth validates the memo word by dereference.
        let _ = crate::object::object_alloc_plain(0);
    }
}

/// #6 (DPA review): every held-word helper answers absent for a retired word
/// whose page was decommitted; none may touch the PROT_NONE page.
#[test]
fn held_word_helpers_are_checked_for_retired_decommitted_rows() {
    use crate::object::shapes as sh;
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    // More than two chunks of dead semantic shapes: at least one whole chunk
    // page has no live record after the trace.
    let words: Vec<u32> = (0..100).map(|_| unrooted_semantic_shape()).collect();
    collect_synchronous_full_trace();
    assert!(
        words.iter().any(|&w| !sh::test_row_committed(w)),
        "no row page was decommitted, the test proves nothing"
    );
    for &w in &words {
        assert!(sh::shape_is_retired(crate::codegen_abi::LayoutWord(w)));
        assert!(sh::shape_descriptor_for_word(crate::codegen_abi::LayoutWord(w)).is_none());
        assert!(sh::shape_record_for_word(crate::codegen_abi::LayoutWord(w)).is_none());
        assert!(sh::shape_object_kind_for_word(crate::codegen_abi::LayoutWord(w)).is_none());
        assert!(sh::shape_brands_for_word(crate::codegen_abi::LayoutWord(w)).is_none());
        assert!(sh::shape_keys_and_proto_for_word(crate::codegen_abi::LayoutWord(w)).is_none());
        assert!(
            sh::shape_live_inline_slot_count_for_word(crate::codegen_abi::LayoutWord(w)).is_none()
        );
        assert_eq!(sh::shape_rep_for_word(crate::codegen_abi::LayoutWord(w)), 0);
    }
}
