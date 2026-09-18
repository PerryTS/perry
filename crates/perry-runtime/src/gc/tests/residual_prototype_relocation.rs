//! An explicit `[[Prototype]]` recorded in the residual registry must survive
//! its owner's relocation, whatever the owner's kind.
//!
//! `Object.setPrototypeOf` keeps a shaped object's prototype in its meta record
//! and everything else — every receiver `meta_capable_object` turns away — in
//! the residual address-keyed registry (`object::prototype_chain`). That entry
//! carries two collector obligations: rekey it when the owner moves, and treat
//! its value as a child edge so the prototype is retained and rewritten. Both
//! were wired to arrays and ordinary objects only, the rekey below
//! `layout_transfer`'s layout-kind return. Every other movable owner — a lazy
//! JSON array, Map, Set, Error, Promise, Date, RegExp — lost its prototype at
//! its first copying minor, and with the rekey alone would have kept a stale
//! address to a prototype that had moved or died.
//!
//! These tests have to be able to fail, so every premise they rest on — the
//! entry landed in the registry, the owner and the prototype really moved — is
//! asserted before the verdict.

use super::super::*;
use super::support::*;

/// Retargeting any array-like latches the process-wide "an array somewhere has
/// a custom `[[Prototype]]`" flag and stands the index fast paths down for the
/// rest of the binary (see `ArrayPrototypeLatchGuard` in `dyn_eval/tests.rs`).
/// Restore what the test found.
struct ArrayPrototypeLatchRestore {
    latch: bool,
    invalidated: u8,
}

impl ArrayPrototypeLatchRestore {
    fn capture() -> Self {
        Self {
            latch: crate::object::prototype_chain::array_static_proto_recorded(),
            invalidated: crate::array::PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED
                .load(std::sync::atomic::Ordering::Relaxed),
        }
    }
}

impl Drop for ArrayPrototypeLatchRestore {
    fn drop(&mut self) {
        crate::object::prototype_chain::test_swap_array_static_proto_recorded(self.latch);
        crate::array::test_swap_array_index_fast_path_invalidated(self.invalidated);
    }
}

/// A small `JSON.parse`-shaped lazy array: small enough that its cluster is
/// born in the nursery (`json_tape::lazy_cluster_is_old`), so a copying minor
/// moves it.
fn nursery_lazy_array(input: &[u8]) -> usize {
    let text = crate::string::js_string_from_bytes(input.as_ptr(), input.len() as u32);
    crate::json_tape::with_built_tape(input, |tape| unsafe {
        crate::json_tape::alloc_lazy_array(
            tape,
            0,
            crate::json_tape::count_array_length(tape, 0),
            text,
        )
    })
    .expect("valid JSON should build a tape") as usize
}

fn obj_type_at(user: usize) -> u8 {
    unsafe { (*header_from_user_ptr(user as *const u8)).obj_type }
}

const MARKER: f64 = 10362.0;

fn marked_prototype() -> usize {
    let proto = crate::object::js_object_alloc(0, 1);
    crate::object::js_object_set_field(proto, 0, crate::value::JSValue::number(MARKER));
    proto as usize
}

fn forget_owners(owners: &[usize]) {
    crate::object::prototype_chain::prune_dead_object_prototype_owners(&|owner| {
        owners.contains(&owner)
    });
}

/// The claim as reported against #10381: a lazy array's prototype, set through
/// the real `Object.setPrototypeOf` entry, across a copying minor that moves
/// both the array and its prototype.
#[test]
fn test_lazy_array_explicit_prototype_survives_a_copying_minor() {
    let _serialized = crate::array::test_serialize();
    let _feedback = crate::typed_feedback::typed_feedback_test_lock();
    let _latch = ArrayPrototypeLatchRestore::capture();
    // Two rooted values — the lazy array and its prototype — so two shadow
    // slots: a store outside the pushed frame is a silent no-op (#7184).
    let _guard = CopyingNurseryTestGuard::new(2);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let lazy = nursery_lazy_array(b"[1,2,3]");
    assert_eq!(
        obj_type_at(lazy),
        GC_TYPE_LAZY_ARRAY,
        "premise: a real lazy array"
    );
    assert!(
        crate::arena::pointer_in_nursery(lazy),
        "premise: the lazy cluster is nursery-born, so a copying minor can move it"
    );
    assert!(crate::gc::gc_type_is_movable(GC_TYPE_LAZY_ARRAY));
    assert!(
        unsafe { crate::object::prototype_chain::meta_capable_object(lazy) }.is_none(),
        "premise: a lazy array has no meta record, so its prototype goes to the registry"
    );

    let proto = marked_prototype();
    js_shadow_slot_set(0, ptr_bits(lazy));
    js_shadow_slot_set(1, ptr_bits(proto));

    // The real user-facing entry, not the recorder beneath it.
    crate::object::js_object_set_prototype_of(
        f64::from_bits(ptr_bits(lazy)),
        f64::from_bits(ptr_bits(proto)),
    );
    let lazy = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let proto = (js_shadow_slot_get(1) & POINTER_MASK) as usize;
    assert_eq!(
        obj_type_at(lazy),
        GC_TYPE_LAZY_ARRAY,
        "premise: setPrototypeOf left the receiver a lazy array"
    );
    assert!(
        crate::object::prototype_chain::test_prototype_registry_latch_armed(),
        "premise: the residual registry holds an entry"
    );
    assert_eq!(
        crate::object::prototype_chain::object_static_prototype(lazy),
        Some(ptr_bits(proto)),
        "premise: the prototype is recorded in the residual registry under the lazy \
         header's own address"
    );
    assert_eq!(
        crate::object::js_object_get_prototype_of(f64::from_bits(ptr_bits(lazy))).to_bits(),
        ptr_bits(proto),
        "premise: Object.getPrototypeOf resolves it before anything moves"
    );

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    let lazy_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let proto_after = (js_shadow_slot_get(1) & POINTER_MASK) as usize;
    assert_ne!(
        lazy_after, lazy,
        "the copying minor must actually relocate the lazy header — an unmoved \
         receiver proves nothing"
    );
    assert_ne!(
        proto_after, proto,
        "the prototype must move too, or a stale recorded address would go unseen"
    );
    assert_eq!(obj_type_at(lazy_after), GC_TYPE_LAZY_ARRAY);

    let recorded = crate::object::prototype_chain::object_static_prototype(lazy_after);
    let resolved =
        crate::object::js_object_get_prototype_of(f64::from_bits(ptr_bits(lazy_after))).to_bits();
    // Leave no entry behind for a later test to trip over, whatever the verdict.
    forget_owners(&[lazy, lazy_after]);

    assert_eq!(
        recorded,
        Some(ptr_bits(proto_after)),
        "the registry entry must follow the lazy header to its new address and name \
         the prototype at ITS current address"
    );
    assert_eq!(
        resolved,
        ptr_bits(proto_after),
        "Object.getPrototypeOf on the relocated lazy array must still return the \
         prototype it was given"
    );
}

/// #10362 SABOTAGE: with `GC_RESIDUAL_PROTO_OWNER` never set, the per-owner
/// gates answer "no entry here" and the collector skips the prototype edge.
///
/// This is the test that proves the bit is load-bearing rather than
/// documentation. Its subject is the same one the two tests around it assert on
/// — the entry must follow the owner and the recorded address must be rewritten
/// — so a green run here with the bit suppressed would mean the gates it feeds
/// are not gating anything, and A' should be withdrawn.
///
/// The membership assertion (`debug_assert_residual_owner_bit`) stands itself
/// down while the sabotage is armed, so this test fails at the real verdict
/// rather than at an assertion the sabotage itself provoked.
#[test]
fn test_suppressing_the_residual_owner_bit_loses_the_prototype() {
    let _serialized = crate::array::test_serialize();
    let _feedback = crate::typed_feedback::typed_feedback_test_lock();
    let _latch = ArrayPrototypeLatchRestore::capture();
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let owner = crate::array::js_array_alloc(4) as usize;
    let obj_type = obj_type_at(owner);
    assert!(
        crate::arena::pointer_in_nursery(owner) && crate::gc::gc_type_is_movable(obj_type),
        "premise: a nursery owner of a movable kind"
    );
    assert_ne!(
        obj_type, GC_TYPE_OBJECT,
        "premise: the subject must be a kind the bit actually gates"
    );
    js_shadow_slot_set(0, ptr_bits(owner));

    let sabotage = crate::object::prototype_chain::residual_proto_bit_sabotage::Guard::arm();
    let proto = marked_prototype();
    crate::object::js_object_set_prototype_of(
        f64::from_bits(ptr_bits(owner)),
        f64::from_bits(ptr_bits(proto)),
    );
    let owner = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_eq!(
        crate::object::prototype_chain::object_static_prototype(owner),
        Some(ptr_bits(proto)),
        "premise: the entry is recorded even with the bit suppressed — the \
         sabotage removes the PROOF, not the entry"
    );
    unsafe {
        let header =
            (owner as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        assert_eq!(
            (*header)._reserved & crate::gc::GC_RESIDUAL_PROTO_OWNER,
            0,
            "premise: the sabotage really did suppress the bit"
        );
    }

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    let owner_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(owner_after, owner, "premise: the owner must actually move");

    let recorded = crate::object::prototype_chain::object_static_prototype(owner_after);
    forget_owners(&[owner, owner_after]);
    js_shadow_slot_set(0, 0);
    drop(sabotage);

    assert!(
        recorded.is_none(),
        "the bit is not load-bearing: with `GC_RESIDUAL_PROTO_OWNER` suppressed the \
         entry still followed its owner, so the per-owner gates in \
         `gc/layout_slot_visit.rs` and `gc/layout/transfer.rs` are not gating \
         anything. A' is documentation — withdraw it."
    );
}

/// Every movable receiver kind that keeps its prototype in the residual
/// registry, with the prototype held by NOTHING but that entry. One copying
/// minor has to rekey the entry, retain the prototype through it, and rewrite
/// the recorded address — the three obligations the registry's population
/// shares regardless of kind. Arrays and ordinary objects are the controls:
/// they were covered before.
#[test]
fn test_residual_prototype_owners_of_every_movable_kind_survive_a_copying_minor() {
    let _serialized = crate::array::test_serialize();
    let _feedback = crate::typed_feedback::typed_feedback_test_lock();
    let _latch = ArrayPrototypeLatchRestore::capture();
    // One rooted owner at a time; its prototype is deliberately unrooted.
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    type Alloc = Box<dyn Fn() -> usize>;
    let mut owners: Vec<(&str, Alloc)> = vec![
        (
            "array",
            Box::new(|| crate::array::js_array_alloc(4) as usize),
        ),
        (
            "object",
            Box::new(|| crate::object::js_object_alloc(0, 1) as usize),
        ),
        ("lazy array", Box::new(|| nursery_lazy_array(b"[1,2,3]"))),
        ("Map", Box::new(|| crate::map::js_map_alloc(0) as usize)),
        ("Set", Box::new(|| crate::set::js_set_alloc(0) as usize)),
        ("Error", Box::new(|| crate::error::js_error_new() as usize)),
        (
            "Promise",
            Box::new(|| crate::promise::js_promise_new() as usize),
        ),
        (
            "Date",
            Box::new(|| (crate::date::js_date_new().to_bits() & POINTER_MASK) as usize),
        ),
    ];
    #[cfg(feature = "regex-engine")]
    owners.push((
        "RegExp",
        Box::new(|| {
            let pattern = crate::string::js_string_from_bytes(b"a+".as_ptr(), 2);
            let flags = crate::string::js_string_from_bytes(b"g".as_ptr(), 1);
            crate::regex::js_regexp_new(pattern, flags) as usize
        }),
    ));

    for (kind, alloc) in owners {
        let owner = alloc();
        let obj_type = obj_type_at(owner);
        assert!(
            crate::arena::pointer_in_nursery(owner) && crate::gc::gc_type_is_movable(obj_type),
            "{kind}: premise: a nursery owner of a movable kind"
        );
        js_shadow_slot_set(0, ptr_bits(owner));
        let proto = marked_prototype();
        crate::object::js_object_set_prototype_of(
            f64::from_bits(ptr_bits(owner)),
            f64::from_bits(ptr_bits(proto)),
        );
        let owner = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
        assert_eq!(
            crate::object::prototype_chain::object_static_prototype(owner),
            Some(ptr_bits(proto)),
            "{kind}: premise: the prototype is recorded for this owner"
        );

        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
        let owner_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
        assert_ne!(
            owner_after, owner,
            "{kind}: premise: the owner must actually move"
        );

        let recorded = crate::object::prototype_chain::object_static_prototype(owner_after);
        forget_owners(&[owner, owner_after]);
        js_shadow_slot_set(0, 0);

        let Some(recorded) = recorded else {
            panic!("{kind}: the registry entry did not follow its owner to the new address");
        };
        let proto_after = (recorded & POINTER_MASK) as usize;
        assert_ne!(
            proto_after, proto,
            "{kind}: the recorded prototype still names its pre-collection address — the \
             prototype was either not retained through the entry or not rewritten"
        );
        assert!(
            crate::arena::pointer_in_nursery(proto_after)
                || crate::arena::pointer_in_old_gen(proto_after),
            "{kind}: the recorded prototype must be a live heap address"
        );
        let marker = crate::object::js_object_get_field(
            proto_after as *const crate::object::ObjectHeader,
            0,
        );
        assert_eq!(
            marker.bits(),
            MARKER.to_bits(),
            "{kind}: the recorded address must hold the prototype that was set"
        );
    }
}
