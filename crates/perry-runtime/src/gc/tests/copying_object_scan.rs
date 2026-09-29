//! The copying drain's plain-object scan (`gc/copying_object_scan.rs`) is a
//! second enumeration of an ordinary object's slots, so it is pinned three ways:
//! it is actually TAKEN by a minor over ordinary objects (a fast path nothing
//! reaches would make every other test here vacuous), every child it visits is
//! evacuated and rewritten, and a plan that drops a slot is REFUSED by the
//! generic-walk cross-check that runs in test and debug-assertion builds.

use super::super::*;
use super::support::*;
use crate::gc::copying_object_scan::sabotage;

fn string_bytes(addr: usize) -> Vec<u8> {
    unsafe {
        let s = addr as *const crate::StringHeader;
        std::slice::from_raw_parts(crate::string::string_data(s), (*s).byte_len as usize).to_vec()
    }
}

const FIELDS: usize = 3;

/// A rooted young object whose every field holds a young string, then a minor.
/// `Ok((plan attempts, every child moved and intact))`; `Err` is the
/// collection thread's panic message.
fn minor_over_a_plain_object(sabotaged: bool) -> Result<(u64, bool), String> {
    minor_over_a_plain_object_with(sabotaged, false)
}

/// `residual_armed`: first give an (old) array an explicit prototype, which
/// arms the residual-prototype registry's process latch — after which the
/// generic walk asks the registry for EVERY ordinary object, and so must this
/// path, without declining.
fn minor_over_a_plain_object_with(
    sabotaged: bool,
    residual_armed: bool,
) -> Result<(u64, bool), String> {
    std::thread::spawn(move || {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _scan = ConservativeScanDisabledGuard::new();
        let _roots = ShadowAndGlobalRootResetGuard;
        if residual_armed {
            let array = unsafe { alloc_old_test_array(1).0 };
            let proto = unsafe { alloc_old_test_object(0).0 } as usize;
            crate::object::prototype_chain::object_set_static_prototype(
                array as usize,
                ptr_bits(proto),
            );
            assert!(
                crate::object::prototype_chain::object_static_prototypes_maybe_nonempty(),
                "premise: the residual registry latch is armed"
            );
        }
        let (parent, fields) = unsafe { alloc_nursery_test_object(FIELDS as u32) };
        let mut children = Vec::new();
        for i in 0..FIELDS {
            let child = young_leaf();
            unsafe { *fields.add(i) = string_bits(child) };
            children.push((child, string_bytes(child)));
        }
        js_shadow_slot_set(0, ptr_bits(parent as usize));
        let before = sabotage::plan_attempts();
        {
            let _sabotage = sabotaged.then(sabotage::DropTopPayloadSlot::arm);
            let _ = gc_collect_minor();
        }
        let attempts = sabotage::plan_attempts() - before;
        let parent_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
        let fields_after = unsafe {
            (parent_after as *const u8).add(std::mem::size_of::<crate::object::ObjectHeader>())
                as *const u64
        };
        let intact = children.iter().enumerate().all(|(i, (old, bytes))| {
            let word = unsafe { *fields_after.add(i) };
            let now = (word & POINTER_MASK) as usize;
            // Checked before any read through `now`: a stale word names from-space.
            now != *old && string_bytes(now) == *bytes
        });
        (attempts, intact)
    })
    .join()
    .map_err(|payload| {
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default()
    })
}

#[test]
fn a_minor_scans_a_plain_object_through_its_plan_and_moves_every_child() {
    let (attempts, intact) = minor_over_a_plain_object(false).expect("minor must not panic");
    assert!(
        attempts > 0,
        "premise: the minor must have taken the plain-object path at least once"
    );
    assert!(
        intact,
        "every field's young child must be evacuated and its word rewritten"
    );
}

#[test]
fn an_armed_residual_prototype_registry_does_not_turn_the_plain_object_path_away() {
    // Recording an ARRAY's prototype also latches the process-wide array
    // prototype flags; restore them, as `dyn_eval/tests.rs`'
    // `ArrayPrototypeLatchGuard` does, or later tests inherit them.
    let _lock = crate::typed_feedback::typed_feedback_test_lock();
    let latch = crate::object::prototype_chain::array_static_proto_recorded();
    let invalidated = crate::array::PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED
        .load(std::sync::atomic::Ordering::Relaxed);
    let outcome = minor_over_a_plain_object_with(false, true);
    crate::object::prototype_chain::test_swap_array_static_proto_recorded(latch);
    crate::array::test_swap_array_index_fast_path_invalidated(invalidated);
    let (attempts, intact) = outcome.expect("minor must not panic");
    assert!(
        attempts > 0,
        "the plain-object path must still scan with the latch armed"
    );
    assert!(
        intact,
        "every field's young child must be evacuated and its word rewritten"
    );
}

#[test]
fn a_plan_that_drops_a_payload_slot_is_refused_by_the_generic_walk_cross_check() {
    let outcome = minor_over_a_plain_object(true);
    assert!(
        matches!(&outcome, Err(message) if message.contains("copying_object_scan")),
        "a plan missing the top payload slot must be caught by the cross-check; got {outcome:?}"
    );
}

/// A young object whose shape has never had an old carrier, promoted IN PLACE
/// by a traced minor — so it is scanned at an old address, and (the malloc
/// registry being empty) the cycle skips the remembered-set rebuild whose
/// generic walk would make the note itself.
/// Returns whether its shape record was noted as old-carried, `(before,
/// after)`: the note the plain-object path skips only when it would change
/// nothing.
fn promoted_receiver_notes_its_shape(sabotaged: bool) -> (bool, bool) {
    std::thread::spawn(move || {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _promote = InPlacePromotionTestGuard::enabled(1000);
        let _scan = ConservativeScanDisabledGuard::new();
        let (parent, fields) = unsafe { alloc_nursery_test_object(2) };
        unsafe { *fields = string_bits(young_leaf()) };
        js_shadow_slot_set(0, ptr_bits(parent as usize));
        let noted = |obj: usize| unsafe {
            crate::object::shapes::old_generation_carrier_already_noted(
                crate::object::shapes::object_shape_record(
                    obj as *const crate::object::ObjectHeader,
                ),
            )
        };
        let before = noted(parent as usize);
        {
            let _no_cross_check = sabotage::NoCrossCheck::arm();
            let _sabotage = sabotaged.then(sabotage::ClaimNotedCarriers::arm);
            let attempts = sabotage::plan_attempts();
            // Not `collect_minor_trace`: a traced cycle arms the layout-scan
            // trace, which the plain-object path declines.
            let _ = gc_collect_minor();
            assert!(
                sabotage::plan_attempts() > attempts,
                "premise: the plain-object path scanned the promoted receiver"
            );
        }
        assert!(
            (js_shadow_slot_get(0) & POINTER_MASK) as usize == parent as usize
                && crate::arena::pointer_in_old_gen(parent as usize),
            "premise: the receiver was promoted where it stood"
        );
        (before, noted(parent as usize))
    })
    .join()
    .expect("carrier-note test thread must not panic")
}

#[test]
fn a_promoted_receiver_notes_its_shape_as_old_carried() {
    assert_eq!(
        promoted_receiver_notes_its_shape(false),
        (false, true),
        "a fresh shape starts un-noted and its promoted carrier must note it"
    );
}

#[test]
fn claiming_every_shape_already_noted_loses_the_old_carrier_note() {
    assert_eq!(
        promoted_receiver_notes_its_shape(true),
        (false, false),
        "with the skip claiming every shape noted, the note never runs"
    );
}
