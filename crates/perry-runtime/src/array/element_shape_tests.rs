//! #7480 element-shape invariant: set / keep / clear across the whole
//! invalidation matrix.
//!
//! Every conjunct of the invariant gets its own named test, so a regression
//! names the rule it broke rather than "an assert failed". The GC-survival
//! half (the bit and the record riding a real copying minor) lives with the
//! other layout-trace tests, in `gc/tests/layout_trace/element_shape.rs` —
//! that is where the copying-nursery guards are.
//!
//! **Both files serialize on `element_shape::ELEMENT_SHAPE_TEST_LOCK`**, taken
//! as the FIRST statement of every test so unwind order drops the
//! state-restoring `…TestGuard`s while it is still held. `ELEMENT_SHAPES` is
//! thread-local and therefore already isolated per test thread, but
//! `ELEMENT_SHAPE_EPOCH` / `CLASS_SHAPE_GENERATION` / `ELEMENT_SHAPE_PROOF_SEQ`
//! are process-wide, so without the lock one test's clear is another test's
//! observation. The lock is poison-tolerant: a panicking test must not
//! cascade into every other one (#7490's ENV_LOCK, #7492's fix).

use super::*;
use crate::array::{
    js_array_alloc, js_array_delete, js_array_push_f64, js_array_set_f64, js_array_set_length,
};

/// Two distinct shaped classes, chosen well clear of the ids the runtime
/// registers for itself.
const CLASS_A: u32 = 0x0007_4801;
const CLASS_B: u32 = 0x0007_4802;

fn instance(class_id: u32) -> f64 {
    let obj = crate::object::js_object_alloc(class_id, 2);
    crate::value::js_nanbox_pointer(obj as i64)
}

fn push(arr: *mut ArrayHeader, value: f64) -> *mut ArrayHeader {
    js_array_push_f64(arr, value)
}

/// `const rows = []; rows.push(new C())` — the construction shape the
/// compile-time collector already admits (#7034 E1/E2), and the one the
/// measured kernel uses.
fn built_from_pushes(class_id: u32, count: usize) -> *mut ArrayHeader {
    let mut arr = js_array_alloc(count as u32);
    for _ in 0..count {
        arr = push(arr, instance(class_id));
    }
    arr
}

fn proof(arr: *mut ArrayHeader) -> Option<ElementShapeProof> {
    unsafe { element_shape_proof(arr) }
}

// ---------------------------------------------------------------------------
// SET
// ---------------------------------------------------------------------------

#[test]
fn first_push_of_a_shaped_object_into_an_empty_array_sets_the_invariant() {
    let _serialized = test_serialize();
    let arr = built_from_pushes(CLASS_A, 1);
    let proof = proof(arr).expect("first shaped push must establish the invariant");
    assert_eq!(proof.class_id, CLASS_A);
    assert_eq!(proof.verified_len, 1);
    unsafe { assert!(test_element_shape_bit_set(arr)) };
    assert!(test_element_shape_record_exists(arr as usize));
}

#[test]
fn matching_pushes_extend_the_verified_prefix() {
    let _serialized = test_serialize();
    let arr = built_from_pushes(CLASS_A, 8);
    let proof = proof(arr).expect("homogeneous pushes must keep the invariant");
    assert_eq!(proof.class_id, CLASS_A);
    assert_eq!(proof.verified_len, 8);
    assert_eq!(unsafe { (*arr).length }, 8);
}

#[test]
fn a_scan_establishes_the_invariant_for_an_array_built_outside_the_funnels() {
    let _serialized = test_serialize();
    // Direct slot writes, the way an inline array literal's codegen fills a
    // fresh allocation: nothing establishes the invariant on the way in.
    let arr = js_array_alloc(4);
    unsafe {
        let elements = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut u64;
        for i in 0..4 {
            std::ptr::write(elements.add(i), instance(CLASS_A).to_bits());
        }
        (*arr).length = 4;
        clear_element_shape(arr);
        assert!(proof(arr).is_none(), "no funnel ran, so no proof yet");
        let healed = ensure_element_shape(arr).expect("a homogeneous array must self-heal");
        assert_eq!(healed.class_id, CLASS_A);
        assert_eq!(healed.verified_len, 4);
    }
}

#[test]
fn ensure_declines_an_empty_array() {
    let _serialized = test_serialize();
    let arr = js_array_alloc(0);
    assert!(
        unsafe { ensure_element_shape(arr) }.is_none(),
        "an empty array's element shape is vacuous and must not be claimed"
    );
    unsafe { assert!(!test_element_shape_bit_set(arr)) };
}

#[test]
fn ensure_declines_a_mixed_array() {
    let _serialized = test_serialize();
    let mut arr = js_array_alloc(2);
    arr = push(arr, instance(CLASS_A));
    arr = push(arr, instance(CLASS_B));
    assert!(unsafe { ensure_element_shape(arr) }.is_none());
}

#[test]
fn ensure_is_idempotent_and_does_not_rescan_a_live_proof() {
    let _serialized = test_serialize();
    let arr = built_from_pushes(CLASS_A, 3);
    let first = unsafe { ensure_element_shape(arr) }.expect("already proven");
    let second = unsafe { ensure_element_shape(arr) }.expect("still proven");
    assert_eq!(first, second);
}

// ---------------------------------------------------------------------------
// CLEAR — value-shaped
// ---------------------------------------------------------------------------

#[test]
fn a_push_of_a_different_class_clears_the_invariant() {
    let _serialized = test_serialize();
    let mut arr = built_from_pushes(CLASS_A, 3);
    assert!(proof(arr).is_some());
    arr = push(arr, instance(CLASS_B));
    assert!(
        proof(arr).is_none(),
        "a mismatched element must retire the proof"
    );
    unsafe { assert!(!test_element_shape_bit_set(arr)) };
}

#[test]
fn a_push_of_a_non_pointer_clears_the_invariant() {
    let _serialized = test_serialize();
    let mut arr = built_from_pushes(CLASS_A, 3);
    arr = push(arr, 42.0);
    assert!(proof(arr).is_none());
}

#[test]
fn an_in_bounds_overwrite_with_a_matching_shape_keeps_the_invariant() {
    let _serialized = test_serialize();
    let arr = built_from_pushes(CLASS_A, 4);
    let before = proof(arr).expect("proven");
    js_array_set_f64(arr, 2, instance(CLASS_A));
    let after = proof(arr).expect("a same-class overwrite must keep the proof");
    assert_eq!(after.class_id, CLASS_A);
    assert_eq!(after.verified_len, 4);
    assert_eq!(after.epoch, before.epoch, "the proof itself is unchanged");
}

#[test]
fn an_in_bounds_overwrite_with_a_different_shape_clears_the_invariant() {
    let _serialized = test_serialize();
    let arr = built_from_pushes(CLASS_A, 4);
    js_array_set_f64(arr, 1, instance(CLASS_B));
    assert!(proof(arr).is_none());
}

#[test]
fn an_in_bounds_overwrite_with_a_number_clears_the_invariant() {
    let _serialized = test_serialize();
    let arr = built_from_pushes(CLASS_A, 4);
    js_array_set_f64(arr, 1, 7.0);
    assert!(proof(arr).is_none());
}

#[test]
fn re_establishing_after_a_clear_draws_a_fresh_proof_identity() {
    let _serialized = test_serialize();
    let mut arr = built_from_pushes(CLASS_A, 2);
    let before = proof(arr).expect("proven").epoch;
    js_array_set_f64(arr, 0, 1.0);
    assert!(proof(arr).is_none());
    // Rebuild a homogeneous array at the same address and re-prove it.
    js_array_set_f64(arr, 0, instance(CLASS_A));
    js_array_set_f64(arr, 1, instance(CLASS_A));
    arr = crate::array::header::clean_arr_ptr_mut(arr);
    let after = unsafe { ensure_element_shape(arr) }.expect("homogeneous again");
    assert_ne!(
        after.epoch, before,
        "a consumer must be able to tell a re-established proof from the retired one"
    );
}

// ---------------------------------------------------------------------------
// CLEAR — structural (holes, length)
// ---------------------------------------------------------------------------

#[test]
fn deleting_an_element_clears_the_invariant() {
    let _serialized = test_serialize();
    let arr = built_from_pushes(CLASS_A, 4);
    assert_eq!(js_array_delete(arr, 1), 1);
    assert!(
        proof(arr).is_none(),
        "a hole breaks 'every element is an object of class C'"
    );
}

#[test]
fn truncating_the_length_clears_the_invariant() {
    let _serialized = test_serialize();
    let arr = built_from_pushes(CLASS_A, 4);
    js_array_set_length(arr, 2.0);
    assert!(proof(arr).is_none());
}

#[test]
fn extending_the_length_with_holes_clears_the_invariant() {
    let _serialized = test_serialize();
    let arr = built_from_pushes(CLASS_A, 2);
    js_array_set_length(arr, 6.0);
    assert!(proof(arr).is_none());
}

#[test]
fn a_length_change_behind_the_runtimes_back_fails_the_proof_closed() {
    let _serialized = test_serialize();
    // The structural half of the matrix: nothing calls a funnel here, the
    // record's pinned `verified_len` is what catches it. This is what makes
    // `pop` and codegen's inline append safe without call sites of their own.
    let arr = built_from_pushes(CLASS_A, 4);
    assert!(proof(arr).is_some());
    unsafe { (*arr).length = 3 };
    assert!(
        proof(arr).is_none(),
        "a length that no longer matches the verified prefix must fail closed"
    );
}

#[test]
fn a_bulk_mutator_rebuild_clears_the_invariant() {
    let _serialized = test_serialize();
    // `shift`/`unshift`/`splice`/`fill`/`copyWithin`/`reverse`/`sort` all
    // mutate slots with bare writes and then land in `rebuild_array_layout`.
    let arr = built_from_pushes(CLASS_A, 4);
    assert!(proof(arr).is_some());
    unsafe { crate::array::header::rebuild_array_layout(arr) };
    assert!(proof(arr).is_none());
}

#[test]
fn declaring_a_numeric_layout_clears_the_invariant() {
    let _serialized = test_serialize();
    let arr = built_from_pushes(CLASS_A, 2);
    assert!(proof(arr).is_some());
    unsafe {
        crate::array::header::set_array_numeric_layout(
            arr,
            crate::array::header::NumericArrayLayout::RawF64,
        )
    };
    assert!(
        proof(arr).is_none(),
        "the numeric and element-shape invariants are mutually exclusive"
    );
}

// ---------------------------------------------------------------------------
// CLEAR — class-level (prototype surgery)
// ---------------------------------------------------------------------------

#[test]
fn prototype_surgery_retires_every_outstanding_proof() {
    let _serialized = test_serialize();
    let a = built_from_pushes(CLASS_A, 2);
    let b = built_from_pushes(CLASS_B, 2);
    assert!(proof(a).is_some());
    assert!(proof(b).is_some());

    let name = b"patched";
    unsafe {
        crate::object::js_register_prototype_method(
            CLASS_A,
            name.as_ptr(),
            name.len(),
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
    }

    assert!(
        proof(a).is_none(),
        "a prototype write must retire the patched class's proofs"
    );
    assert!(
        proof(b).is_none(),
        "the generation bump is global — conservative in the safe direction"
    );
    // …and the array self-heals if it is still homogeneous.
    assert_eq!(
        unsafe { ensure_element_shape(b) }.map(|p| p.class_id),
        Some(CLASS_B)
    );
}

// ---------------------------------------------------------------------------
// EPOCH
// ---------------------------------------------------------------------------

#[test]
fn a_clear_advances_the_global_epoch_and_a_keep_preserves_the_proof_identity() {
    let _serialized = test_serialize();
    let arr = built_from_pushes(CLASS_A, 3);
    let quiet = element_shape_epoch();
    let pinned = proof(arr).expect("proven").epoch;

    // A matching overwrite retires nothing. The *per-array* identity is the
    // deterministic half of this assertion — it lives in the thread-local
    // record, so no concurrent test can move it. The shared lock keeps other
    // element-shape tests out, but the global counter is also bumped by
    // prototype surgery anywhere in the suite, so asserting it "held still"
    // would be asserting something this test does not control.
    js_array_set_f64(arr, 0, instance(CLASS_A));
    assert_eq!(
        proof(arr).expect("still proven").epoch,
        pinned,
        "a keep must not retire the proof a consumer pinned"
    );

    // A mismatch does retire it, and that has to be visible in the global
    // word a hoisted guard re-reads.
    js_array_set_f64(arr, 0, instance(CLASS_B));
    assert!(proof(arr).is_none());
    assert!(
        element_shape_epoch() > quiet,
        "a clear must advance the word a hoisted guard re-reads"
    );
}

#[test]
fn the_check_helper_pins_class_and_proof_identity() {
    let _serialized = test_serialize();
    let arr = built_from_pushes(CLASS_A, 3);
    let proof = proof(arr).expect("proven");
    assert_eq!(
        crate::array::js_array_element_shape_check(arr, proof.class_id as i32, proof.epoch as i64),
        1
    );
    assert_eq!(
        crate::array::js_array_element_shape_check(arr, CLASS_B as i32, proof.epoch as i64),
        0,
        "a different class must not validate"
    );
    assert_eq!(
        crate::array::js_array_element_shape_check(
            arr,
            proof.class_id as i32,
            proof.epoch as i64 + 1
        ),
        0,
        "a stale proof identity must not validate"
    );
}

#[test]
fn the_ffi_query_reports_zero_for_an_unproven_array() {
    let _serialized = test_serialize();
    let arr = js_array_alloc(0);
    assert_eq!(crate::array::js_array_element_shape_class(arr), 0);
    assert_eq!(crate::array::js_array_element_shape_version(arr), -1);
    assert_eq!(crate::array::js_array_ensure_element_shape(arr), 0);
}

// ---------------------------------------------------------------------------
// Relocation
// ---------------------------------------------------------------------------

#[test]
fn growth_forwarding_carries_the_invariant_to_the_new_backing() {
    let _serialized = test_serialize();
    // `js_array_grow` copies `_reserved` verbatim and calls `layout_transfer`,
    // which is where `transfer_element_shape` hangs.
    let arr = built_from_pushes(CLASS_A, 2);
    let before = proof(arr).expect("proven");
    let grown = crate::array::js_array_grow(arr, 512);
    assert_ne!(grown as usize, arr as usize, "the array must actually move");
    let after = proof(grown).expect("the proof must follow the storage");
    assert_eq!(after, before);
    assert!(test_element_shape_record_exists(grown as usize));
    assert!(!test_element_shape_record_exists(arr as usize));
}

#[test]
fn a_transfer_without_a_record_fails_the_destination_closed() {
    let _serialized = test_serialize();
    let src = built_from_pushes(CLASS_A, 2);
    let dst = built_from_pushes(CLASS_A, 2);
    // Simulate the hazard the bit-is-authority rule exists for: the bit rides
    // a move whose record did not.
    test_clear_element_shape_table();
    transfer_element_shape(src as usize, dst as usize);
    assert!(
        proof(dst).is_none(),
        "a bit with no record must never read as a proof"
    );
    unsafe { assert!(!test_element_shape_bit_set(dst)) };
}

#[test]
fn a_fail_closed_transfer_leaves_no_record_for_the_next_array_to_inherit() {
    // The destination of a fail-closed transfer must be left with NO record,
    // not merely a cleared bit. A survivor there is exactly what a later
    // establishment could read an identity out of, which would silently
    // continue a *different* array's proof identity — defeating the very
    // versioning a consumer guards on. Two independent defences are asserted:
    // the record is gone, AND establishing draws a fresh identity anyway.
    let _serialized = test_serialize();
    let src = built_from_pushes(CLASS_A, 2);
    let dst = built_from_pushes(CLASS_A, 2);
    let doomed = proof(dst).expect("proven").epoch;

    // `src` proves nothing, so the transfer must fail closed at `dst`.
    unsafe { clear_element_shape(src) };
    transfer_element_shape(src as usize, dst as usize);

    assert!(
        !test_element_shape_record_exists(dst as usize),
        "a fail-closed transfer must not leave an orphan record at the destination"
    );
    unsafe { assert!(!test_element_shape_bit_set(dst)) };

    // Now establish at that very address, as a recycled allocation would.
    let reproven = unsafe { ensure_element_shape(dst) }.expect("still homogeneous");
    assert_ne!(
        reproven.epoch, doomed,
        "a re-established proof must not continue the retired proof's identity"
    );
}

// ---------------------------------------------------------------------------
// Lifecycle hooks — what stops a recycled address inheriting a stale identity
// ---------------------------------------------------------------------------

#[test]
fn forget_element_shape_removes_the_record_on_address_recycling() {
    // Reached from `gc::layout_clear_for_ptr` on object death — the only path
    // that REMOVES the record rather than clearing the bit.
    let _serialized = test_serialize();
    let arr = built_from_pushes(CLASS_A, 3);
    let retired = proof(arr).expect("proven").epoch;
    assert!(test_element_shape_record_exists(arr as usize));

    forget_element_shape(arr as usize);

    assert!(
        !test_element_shape_record_exists(arr as usize),
        "a dead allocation's record must not outlive it"
    );
    unsafe { assert!(!test_element_shape_bit_set(arr)) };
    assert!(proof(arr).is_none());

    let reused = unsafe { ensure_element_shape(arr) }.expect("still homogeneous");
    assert_ne!(
        reused.epoch, retired,
        "an address reused after a death must not inherit the dead proof's identity"
    );
}

#[test]
fn pruning_dead_owners_removes_their_records() {
    // `gc::dead_owner::fan_out` calls this with the collection's liveness
    // predicate, on the same hook that prunes `ARRAY_NAMED_PROPS`.
    let _serialized = test_serialize();
    let dead = built_from_pushes(CLASS_A, 2);
    let live = built_from_pushes(CLASS_A, 2);
    assert!(test_element_shape_record_exists(dead as usize));
    assert!(test_element_shape_record_exists(live as usize));

    let dead_key = dead as usize;
    prune_dead_element_shape_owners(&|owner| owner == dead_key);

    assert!(
        !test_element_shape_record_exists(dead as usize),
        "a provably dead owner's record must be pruned"
    );
    assert!(
        test_element_shape_record_exists(live as usize),
        "a live owner's record must survive the prune"
    );
    assert!(
        proof(live).is_some(),
        "pruning must not disturb a live array's proof"
    );
}
