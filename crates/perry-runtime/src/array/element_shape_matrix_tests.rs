//! #7480 element-shape invariant: the **end-to-end revocation matrix**.
//!
//! `element_shape_tests.rs` covers the invariant's own funnels
//! (`note_element_store`, `clear_element_shape`, `transfer_element_shape`) and
//! asserts that `rebuild_array_layout` drops a proof. What it does *not*
//! assert is that the mutators actually **reach** those funnels: its bulk-
//! mutator test calls `rebuild_array_layout` directly, so it proves the proxy,
//! not the subject. That is CLAUDE.md's fourth way a gate cannot fail — "the
//! gate runs but its subject never did" — and it is the one shape that would
//! let a mutator quietly stop revoking while the suite stayed green.
//!
//! This file closes that hole. Every test here drives a **real FFI entry
//! point** against a shaped array and asserts the documented verdict. Deleting
//! any single `clear_element_shape` / `rebuild_array_layout` call from the
//! runtime turns at least one test here red.
//!
//! ## The contract, stated once
//!
//! Three verdicts, and every mutation site is exactly one of them:
//!
//! * **MAINTAIN** — the proof survives with its identity intact. Only two
//!   sites qualify: an in-bounds store whose value has the proven class, and a
//!   contiguous append of the proven class (which also extends `verified_len`).
//! * **REVOKE** — the proof is dropped and the global epoch advances. Every
//!   bulk mutator, conservatively, plus any store of a non-conforming value.
//! * **STRUCTURAL REVOKE** — no call site of its own; the record's pinned
//!   `verified_len` stops matching `length`, so the next query fails closed.
//!   This is what covers `pop`, and any future length-changing path nobody
//!   remembers to hook.
//!
//! A conservative REVOKE is not the same as heterogeneity, so each bulk
//! mutator that is a **permutation or a same-class rewrite** additionally
//! asserts it **re-proves** on the next `ensure_element_shape` — with a *fresh*
//! identity, because a consumer that pinned the old one must not silently ride
//! through. That is the self-healing half, and it is what stops a future
//! "optimisation" from replacing a revoke with a no-op and calling the
//! still-green suite evidence.

use super::*;
use crate::array::{
    js_array_alloc, js_array_copy_within, js_array_delete, js_array_fill, js_array_fill_range,
    js_array_pop_f64, js_array_push_f64, js_array_reverse, js_array_set_f64,
    js_array_set_f64_unchecked, js_array_set_length, js_array_shift_f64, js_array_sort_default,
    js_array_splice, js_array_unshift_f64,
};

/// Two distinct shaped classes, matching the ids `element_shape_tests` uses.
const CLASS_A: u32 = 0x0007_4801;
const CLASS_B: u32 = 0x0007_4802;

fn instance(class_id: u32) -> f64 {
    let obj = crate::object::js_object_alloc(class_id, 2);
    crate::value::js_nanbox_pointer(obj as i64)
}

/// `const rows = []; rows.push(new C())` — the construction shape the measured
/// kernel uses, and the one `note_element_store` establishes from.
fn shaped(count: usize) -> *mut ArrayHeader {
    let mut arr = js_array_alloc(count as u32);
    for _ in 0..count {
        arr = js_array_push_f64(arr, instance(CLASS_A));
    }
    assert!(
        unsafe { element_shape_proof(arr) }.is_some(),
        "fixture must start proven, or every verdict below is vacuous"
    );
    arr
}

fn proof(arr: *mut ArrayHeader) -> Option<ElementShapeProof> {
    unsafe { element_shape_proof(arr) }
}

/// Assert a mutation **revoked** the proof, and that the global epoch moved.
///
/// The epoch check is what makes this a real assertion rather than "the query
/// happened to return `None`": a consumer's hoisted guard re-reads that word,
/// so a revoke the epoch does not advertise is a revoke the consumer misses.
fn assert_revoked(arr: *mut ArrayHeader, before: u64, family: &str) {
    assert!(
        proof(arr).is_none(),
        "{family} must revoke the element-shape proof"
    );
    assert!(
        element_shape_epoch() > before,
        "{family} revoked but never advanced the epoch — a hoisted guard would not notice"
    );
}

/// Assert the array is still homogeneous underneath, so the conservative
/// revoke self-heals — and that healing mints a **fresh** identity.
fn assert_reproves(arr: *mut ArrayHeader, retired: u64, family: &str) {
    let healed = unsafe { ensure_element_shape(arr) }.unwrap_or_else(|| {
        panic!("{family} is a permutation/same-class rewrite; it must re-prove")
    });
    assert_eq!(
        healed.class_id, CLASS_A,
        "{family} re-proved the wrong class"
    );
    assert_ne!(
        healed.epoch, retired,
        "{family} re-proved but reused the retired identity — a consumer that pinned it would ride through a break"
    );
}

// ---------------------------------------------------------------------------
// MAINTAIN — the only two sites that keep a proof
// ---------------------------------------------------------------------------

#[test]
fn matrix_in_bounds_store_of_the_proven_class_maintains() {
    let _serialized = test_serialize();
    let arr = shaped(3);
    let pinned = proof(arr).unwrap();
    js_array_set_f64(arr, 1, instance(CLASS_A));
    let after = proof(arr).expect("a same-class in-bounds store must maintain");
    assert_eq!(after.class_id, CLASS_A);
    assert_eq!(
        after.epoch, pinned.epoch,
        "maintaining must not retire the proof identity"
    );
    assert_eq!(after.verified_len, 3);
}

#[test]
fn matrix_contiguous_append_of_the_proven_class_maintains_and_extends() {
    let _serialized = test_serialize();
    let arr = shaped(2);
    let pinned = proof(arr).unwrap();
    let arr = js_array_push_f64(arr, instance(CLASS_A));
    let after = proof(arr).expect("a same-class append must maintain");
    assert_eq!(after.epoch, pinned.epoch);
    assert_eq!(after.verified_len, 3, "the verified prefix must extend");
}

/// The *unchecked* store is codegen's fast path and skips the bounds/extend
/// logic — but not `note_array_slot`, so it must still maintain and revoke.
#[test]
fn matrix_unchecked_store_maintains_on_class_match() {
    let _serialized = test_serialize();
    let arr = shaped(3);
    let pinned = proof(arr).unwrap();
    js_array_set_f64_unchecked(arr, 2, instance(CLASS_A));
    let after = proof(arr).expect("the unchecked store must reach the funnel too");
    assert_eq!(after.epoch, pinned.epoch);
}

#[test]
fn matrix_unchecked_store_revokes_on_class_mismatch() {
    let _serialized = test_serialize();
    let arr = shaped(3);
    let before = element_shape_epoch();
    js_array_set_f64_unchecked(arr, 2, instance(CLASS_B));
    assert_revoked(arr, before, "an unchecked store of a different class");
}

// ---------------------------------------------------------------------------
// REVOKE — value-driven
// ---------------------------------------------------------------------------

#[test]
fn matrix_in_bounds_store_of_a_different_class_revokes() {
    let _serialized = test_serialize();
    let arr = shaped(3);
    let before = element_shape_epoch();
    js_array_set_f64(arr, 1, instance(CLASS_B));
    assert_revoked(arr, before, "a different-class store");
}

#[test]
fn matrix_store_of_a_primitive_revokes() {
    let _serialized = test_serialize();
    let arr = shaped(3);
    let before = element_shape_epoch();
    js_array_set_f64(arr, 1, 42.0);
    assert_revoked(arr, before, "a primitive store");
}

#[test]
fn matrix_append_of_a_different_class_revokes() {
    let _serialized = test_serialize();
    let arr = shaped(2);
    let before = element_shape_epoch();
    let arr = js_array_push_f64(arr, instance(CLASS_B));
    assert_revoked(arr, before, "a different-class append");
}

#[test]
fn matrix_delete_revokes() {
    let _serialized = test_serialize();
    let arr = shaped(4);
    let before = element_shape_epoch();
    js_array_delete(arr, 1);
    assert_revoked(arr, before, "`delete arr[i]`");
}

// ---------------------------------------------------------------------------
// REVOKE — bulk mutators, driven through their real entry points
//
// Each also asserts self-healing where the operation is a permutation or a
// same-class rewrite: a conservative revoke must not be mistaken for the array
// having genuinely stopped being homogeneous.
// ---------------------------------------------------------------------------

/// The three tests below additionally pin **pointer identity**. None of these
/// operations can reallocate, so the receiver they return must be the array
/// that was proven — without that assertion, an implementation that returned a
/// fresh (and therefore trivially unproven) array would satisfy `assert_revoked`
/// while revoking nothing, which is the vacuous-pass shape this whole file
/// exists to rule out.
#[test]
fn matrix_reverse_revokes_and_reproves() {
    let _serialized = test_serialize();
    let arr = shaped(4);
    let retired = proof(arr).unwrap().epoch;
    let before = element_shape_epoch();
    let out = js_array_reverse(arr);
    assert!(
        std::ptr::eq(out, arr),
        "`reverse` is in-place; a fresh array would make the verdict vacuous"
    );
    assert_revoked(out, before, "`reverse`");
    assert_reproves(out, retired, "`reverse`");
}

/// `sort`'s default path is a rank permutation written back through
/// `RootedArrayElems::set`, so it revokes via the **store** funnel rather than
/// `rebuild_array_layout` — verified by sabotage: removing the revoke from
/// `rebuild_array_layout` leaves this test green, removing it from
/// `layout_note_slot` turns it red. Defence in depth, not redundancy.
#[test]
fn matrix_sort_revokes_and_reproves() {
    let _serialized = test_serialize();
    let arr = shaped(4);
    let retired = proof(arr).unwrap().epoch;
    let before = element_shape_epoch();
    let out = js_array_sort_default(arr);
    assert!(
        std::ptr::eq(out, arr),
        "`sort` returns its receiver; a fresh array would make the verdict vacuous"
    );
    assert_revoked(out, before, "`sort`");
    assert_reproves(out, retired, "`sort`");
}

#[test]
fn matrix_copy_within_revokes_and_reproves() {
    let _serialized = test_serialize();
    let arr = shaped(4);
    let retired = proof(arr).unwrap().epoch;
    let before = element_shape_epoch();
    let out = js_array_copy_within(arr, 0.0, 2.0, 0, 0.0);
    assert!(
        std::ptr::eq(out, arr),
        "`copyWithin` is in-place; a fresh array would make the verdict vacuous"
    );
    assert_revoked(out, before, "`copyWithin`");
    assert_reproves(out, retired, "`copyWithin`");
}

/// `fill` overwrites every slot, so the proof must go — and because the filler
/// here is a *different* class, the array is genuinely heterogeneous no more:
/// it re-proves as `CLASS_B`, not `CLASS_A`. A revoke that healed back to the
/// old class would be the exact lie a consumer's guard cannot survive.
#[test]
fn matrix_fill_revokes_and_reproves_as_the_filled_class() {
    let _serialized = test_serialize();
    let arr = shaped(4);
    let before = element_shape_epoch();
    let arr = js_array_fill(arr, instance(CLASS_B));
    assert_revoked(arr, before, "`fill`");
    let healed = unsafe { ensure_element_shape(arr) }.expect("a uniformly filled array re-proves");
    assert_eq!(
        healed.class_id, CLASS_B,
        "`fill` must heal to the FILLED class, never back to the retired one"
    );
}

#[test]
fn matrix_fill_range_revokes_leaving_a_genuinely_mixed_array() {
    let _serialized = test_serialize();
    let arr = shaped(4);
    let before = element_shape_epoch();
    let arr = js_array_fill_range(arr, instance(CLASS_B), 0.0, 2.0);
    assert_revoked(arr, before, "`fill(v, start, end)`");
    assert!(
        unsafe { ensure_element_shape(arr) }.is_none(),
        "a partial fill of another class leaves a mixed array; it must NOT re-prove"
    );
}

/// The soundness-critical case: `splice` can replace elements **without
/// changing `length`**, so the structural `verified_len` check cannot catch it.
/// Only splice's own `rebuild_array_layout` can. If that call is ever dropped,
/// this is the test that goes red — and nothing else would.
#[test]
fn matrix_splice_equal_length_replacement_revokes() {
    let _serialized = test_serialize();
    let arr = shaped(4);
    let len_before = unsafe { (*arr).length };
    let before = element_shape_epoch();
    let items = [instance(CLASS_B)];
    let mut out: *mut ArrayHeader = arr;
    js_array_splice(arr, 1, 1, items.as_ptr(), 1, &mut out);
    assert_eq!(
        unsafe { (*out).length },
        len_before,
        "this test is only meaningful while the length is UNCHANGED — \
         otherwise the structural check would catch it and splice's own \
         revoke would go untested"
    );
    assert_revoked(out, before, "an equal-length `splice` replacement");
    assert!(
        unsafe { ensure_element_shape(out) }.is_none(),
        "the spliced-in element really is a different class"
    );
}

#[test]
fn matrix_splice_pure_delete_revokes_and_reproves() {
    let _serialized = test_serialize();
    let arr = shaped(4);
    let retired = proof(arr).unwrap().epoch;
    let before = element_shape_epoch();
    let mut out: *mut ArrayHeader = arr;
    js_array_splice(arr, 1, 1, std::ptr::null(), 0, &mut out);
    assert_revoked(out, before, "a deleting `splice`");
    assert_reproves(out, retired, "a deleting `splice`");
}

#[test]
fn matrix_unshift_revokes_and_reproves() {
    let _serialized = test_serialize();
    let arr = shaped(3);
    let retired = proof(arr).unwrap().epoch;
    let before = element_shape_epoch();
    let arr = js_array_unshift_f64(arr, instance(CLASS_A));
    assert_revoked(arr, before, "`unshift`");
    assert_reproves(arr, retired, "`unshift`");
}

#[test]
fn matrix_shift_revokes_and_reproves() {
    let _serialized = test_serialize();
    let arr = shaped(3);
    let retired = proof(arr).unwrap().epoch;
    let before = element_shape_epoch();
    js_array_shift_f64(arr);
    assert_revoked(arr, before, "`shift`");
    assert_reproves(arr, retired, "`shift`");
}

// ---------------------------------------------------------------------------
// STRUCTURAL REVOKE — no call site of its own; `verified_len` catches it
// ---------------------------------------------------------------------------

/// `pop` is deliberately un-hooked: it shortens `length`, and the record's
/// pinned `verified_len` stops matching. This test exists to pin that the
/// structural half is load-bearing, not decorative — it is the mechanism that
/// covers every length-changing path nobody remembers to hook, including
/// codegen's inline append.
#[test]
fn matrix_pop_revokes_structurally_then_reproves() {
    let _serialized = test_serialize();
    let arr = shaped(4);
    let retired = proof(arr).unwrap().epoch;
    js_array_pop_f64(arr);
    assert!(
        proof(arr).is_none(),
        "`pop` must fail the proof closed on the length mismatch"
    );
    assert_reproves(arr, retired, "`pop`");
}

#[test]
fn matrix_length_truncate_and_extend_revoke() {
    let _serialized = test_serialize();
    let truncated = shaped(4);
    js_array_set_length(truncated, 2.0);
    assert!(proof(truncated).is_none(), "`arr.length = 2` must revoke");

    let extended = shaped(2);
    js_array_set_length(extended, 6.0);
    assert!(
        proof(extended).is_none(),
        "`arr.length = 6` leaves holes; it must revoke"
    );
    assert!(
        unsafe { ensure_element_shape(extended) }.is_none(),
        "a hole-tailed array must not re-prove"
    );
}

// ---------------------------------------------------------------------------
// BUILDERS — the "rebuild regains it" half of self-healing
//
// Array *builders* split across the two funnels (audited for #7480): the
// per-element ones (`JSON.parse`, the #7539 tape materialiser, most of the
// `Array.from` family, the concat fallback) reach `layout_note_slot` and so
// **establish** as they fill; the bulk-copy ones (dense spread, the concat fast
// path, `Array.from` on a jsvalue) `ptr::copy` and then `rebuild_array_layout_
// exact`, which leaves the result deliberately **unproven**.
//
// Both are correct — the rule is that a builder may leave a result unproven,
// but must never leave it proven at the WRONG class. These tests pin exactly
// that, and pin that the unproven case heals on the first `ensure`.
// ---------------------------------------------------------------------------

/// `[...arr]` — the bulk-copy builder. The clone is a fresh allocation that
/// may land at a recycled address, so the interesting assertion is not that it
/// starts unproven but that whatever it reports is about *itself*.
#[test]
fn matrix_spread_build_never_inherits_the_sources_proof() {
    let _serialized = test_serialize();
    let src = shaped(4);
    let src_proof = proof(src).expect("source is proven");

    let clone = crate::array::flat_clone::js_array_clone_for_spread(
        crate::value::js_nanbox_pointer(src as i64),
    );

    if let Some(cloned) = proof(clone) {
        assert_ne!(
            cloned.epoch, src_proof.epoch,
            "the clone must never carry the SOURCE's proof identity — a consumer \
             that pinned it would treat two different arrays as one"
        );
        assert_eq!(cloned.class_id, CLASS_A);
    }
    // Whatever it started as, the clone really is homogeneous, so it proves.
    let healed = unsafe { ensure_element_shape(clone) }.expect("a spread clone re-proves");
    assert_eq!(healed.class_id, CLASS_A);
    assert_ne!(healed.epoch, src_proof.epoch);

    assert_eq!(
        proof(src).map(|p| p.epoch),
        Some(src_proof.epoch),
        "building a clone must not disturb the source's proof"
    );
}

/// The acceptance case stated in #7480: a **revoked** array that is rebuilt
/// regains the invariant. `arr.map(identity)` is the JS spelling; at this layer
/// the equivalent is "copy the elements into a fresh array", which is what the
/// bulk-copy builders do.
#[test]
fn matrix_a_revoked_array_regains_the_invariant_when_rebuilt() {
    let _serialized = test_serialize();
    let arr = shaped(4);

    // Revoke it for real, and confirm it is genuinely gone.
    let before = element_shape_epoch();
    js_array_set_f64(arr, 1, 7.0);
    assert_revoked(arr, before, "a primitive store");
    assert!(
        unsafe { ensure_element_shape(arr) }.is_none(),
        "still holding a primitive, so it must NOT re-prove in place"
    );

    // Rebuild it the way user code does — every element replaced by a shaped
    // instance — and the invariant comes back on its own.
    let mut rebuilt = js_array_alloc(4);
    for _ in 0..4 {
        rebuilt = js_array_push_f64(rebuilt, instance(CLASS_A));
    }
    let healed = proof(rebuilt).expect("a rebuilt homogeneous array is proven again");
    assert_eq!(healed.class_id, CLASS_A);
    assert_eq!(healed.verified_len, 4);
}

/// `Array.from`-family builder that fills per element: it reaches
/// `layout_note_slot`, so a homogeneous source establishes as it fills.
#[test]
fn matrix_from_values_builder_establishes_or_heals_but_never_lies() {
    let _serialized = test_serialize();
    let values = [
        instance(CLASS_A),
        instance(CLASS_A),
        instance(CLASS_A),
        instance(CLASS_A),
    ];
    let built = crate::array::alloc::js_array_from_values(values.as_ptr(), values.len() as u32);
    if let Some(p) = proof(built) {
        assert_eq!(
            p.class_id, CLASS_A,
            "a builder must never prove a wrong class"
        );
    }
    assert_eq!(
        unsafe { ensure_element_shape(built) }.map(|p| p.class_id),
        Some(CLASS_A)
    );

    // The mixed source must not produce a proof at all.
    let mixed = [instance(CLASS_A), instance(CLASS_B)];
    let built_mixed = crate::array::alloc::js_array_from_values(mixed.as_ptr(), mixed.len() as u32);
    assert!(proof(built_mixed).is_none());
    assert!(unsafe { ensure_element_shape(built_mixed) }.is_none());
}

// ---------------------------------------------------------------------------
// The matrix is COMPLETE — a proven array that survives every family untouched
// would mean a family stopped revoking without any single test noticing.
// ---------------------------------------------------------------------------

/// A roll-up that fails if *any* bulk family silently stops revoking.
///
/// The per-family tests above name the rule each site breaks; this one exists
/// so that a change which quietly removes the funnel from several families at
/// once cannot be dismissed as "one flaky test". It also asserts its own
/// subject was live: the fixture is re-proven before every step, so a step that
/// found nothing to revoke fails loudly instead of passing vacuously.
#[test]
fn matrix_every_bulk_family_revokes() {
    let _serialized = test_serialize();

    #[allow(clippy::type_complexity)]
    let families: Vec<(&str, Box<dyn Fn(*mut ArrayHeader) -> *mut ArrayHeader>)> = vec![
        ("reverse", Box::new(|a| js_array_reverse(a))),
        ("sort", Box::new(|a| js_array_sort_default(a))),
        (
            "copyWithin",
            Box::new(|a| js_array_copy_within(a, 0.0, 2.0, 0, 0.0)),
        ),
        ("fill", Box::new(|a| js_array_fill(a, instance(CLASS_A)))),
        (
            "unshift",
            Box::new(|a| js_array_unshift_f64(a, instance(CLASS_A))),
        ),
        (
            "shift",
            Box::new(|a| {
                js_array_shift_f64(a);
                a
            }),
        ),
        (
            "splice",
            Box::new(|a| {
                let mut out: *mut ArrayHeader = a;
                js_array_splice(a, 1, 1, std::ptr::null(), 0, &mut out);
                out
            }),
        ),
    ];

    for (name, op) in families {
        let arr = shaped(4);
        assert!(
            proof(arr).is_some(),
            "{name}: fixture must be proven before the op, or the check is vacuous"
        );
        let before = element_shape_epoch();
        let arr = op(arr);
        assert_revoked(arr, before, name);
    }
}
