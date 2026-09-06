//! #7511 — the contract the codegen-side inline PARENT-generation gate rests on.
//!
//! `perry-codegen`'s `expr::write_barrier::emit_parent_may_need_remembering_check`
//! puts the array push's `js_write_barrier_slot` call behind ONE inline test of
//! the parent's live header byte plus one global:
//!
//! ```text
//! parent_may_need_remembering(parent) :=
//!       (header(parent).gc_flags & GC_FLAG_TENURED) != 0
//!    || PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT != 0
//! ```
//!
//! Two independent obligations, and this file pins both.
//!
//! **1. `Old ⟹ TENURED`.** The remembered set only ever needs an entry when
//! `barrier_parent_needs_remembering` classifies the parent `Old`, so skipping
//! the call on a TENURED-clear header is sound exactly while no live old-gen
//! object can lack the bit. Nothing in the allocator enforces that:
//! `arena_alloc_gc_old` writes `GC_FLAG_ARENA | gc_birth_extra_flags()` and
//! leaves `GC_FLAG_TENURED` to each of its eight callers. A ninth caller that
//! forgets it compiles, passes every existing test, and strands a live child in
//! generated code only. `every_old_gen_birth_path_sets_tenured` is what turns
//! that into a red build, and `barrier_parent_needs_remembering` carries the
//! matching `debug_assert!` so every old-parent store in every debug/test run
//! re-checks it.
//!
//! **2. The incremental clause is not optional.** Skipping the call also skips
//! `barrier_child_prologue`'s `incremental_mark_barrier_value` — the
//! insertion/SATB shading, which is not a generational question at all. A zero
//! count *proves* this thread's `INCREMENTAL_MARK_BARRIER_VALID_PTRS` is null,
//! because `incremental_mark_barrier_enable` increments the count BEFORE
//! installing the thread-local and disable clears the pointer BEFORE
//! decrementing the count; a non-zero count must force the call even for a
//! nursery parent. The emitted LLVM `monotonic` load is this Rust model's
//! `Relaxed` load. `the_incremental_clause_forces_the_call_for_a_young_parent`
//! pins the clause, and fails if it is dropped.

use super::super::*;
use super::support::*;
use std::sync::atomic::Ordering;

/// The exact flag comparand emitted by `emit_parent_may_need_remembering_check`
/// (codegen spells it `"32"`). Kept as a literal so a drift on either side has
/// to be mirrored by hand rather than silently inherited.
const CODEGEN_GC_FLAG_TENURED: u8 = 0x20;

/// The codegen predicate, reproduced exactly.
fn codegen_parent_may_need_remembering(parent_flags: u8, incremental_active: u32) -> bool {
    parent_flags & CODEGEN_GC_FLAG_TENURED != 0 || incremental_active != 0
}

fn header_flags(user_ptr: usize) -> u8 {
    unsafe { (*header_from_user_ptr(user_ptr as *const u8)).gc_flags }
}

#[test]
fn codegen_tenured_comparand_matches_the_runtime_flag() {
    assert_eq!(
        CODEGEN_GC_FLAG_TENURED, GC_FLAG_TENURED,
        "codegen emits `and i8 %gc_flags, {CODEGEN_GC_FLAG_TENURED}` — if the runtime flag moves, \
         the emitted gate silently tests the wrong bit"
    );
}

/// **The invariant, made able to fail.** Every production path that can place a
/// live object at an address `classify_heap_generation` calls `Old` must leave
/// `GC_FLAG_TENURED` set on it.
///
/// Deliberately exercises the birth paths that do NOT go through tenuring —
/// those are the ones where the bit is a caller's remembered obligation rather
/// than a consequence of having survived:
///
/// * the size-independent born-old wrapper (`arena_alloc_gc_old_born_tenured`),
/// * the large-object arm of the ordinary nursery allocator, which diverts
///   anything over `large_object_threshold_for_type` straight into old-gen,
/// * `buffer_alloc`, which is old-gen because its bytes are handed to FFI.
///
/// The survivor/evacuation paths (`gc/copying.rs`, `gc/oldgen.rs`) set the bit
/// in the same expression that selects the old-gen allocation, so they cannot
/// drift apart; these three can.
#[test]
fn every_old_gen_birth_path_sets_tenured() {
    let _guard = GcTestIsolationGuard::new();

    let born_tenured =
        crate::arena::arena_alloc_gc_old_born_tenured(64, 8, GC_TYPE_OBJECT) as usize;
    // Sized from the SAME function the allocator consults, so the fixture
    // follows a future retune of either threshold instead of silently ceasing
    // to be an old-gen birth (which is how this assertion would stop covering
    // its invariant rather than fail).
    let large = crate::arena::arena_alloc_gc(
        crate::gc::large_object_threshold_for_type(GC_TYPE_OBJECT) + 64,
        8,
        GC_TYPE_OBJECT,
    ) as usize;
    let buffer = crate::buffer::buffer_alloc(128) as usize;

    for (label, addr) in [
        ("arena_alloc_gc_old_born_tenured", born_tenured),
        ("large-object birth", large),
        ("buffer_alloc", buffer),
    ] {
        assert_eq!(
            crate::arena::classify_heap_generation(addr),
            crate::arena::HeapGeneration::Old,
            "{label} is supposed to be an OLD-generation birth — if it is not, this test has \
             stopped covering the invariant it exists for"
        );
        assert_ne!(
            header_flags(addr) & GC_FLAG_TENURED,
            0,
            "{label} produced a LIVE old-gen object with GC_FLAG_TENURED clear. The #7511 inline \
             gate skips the write barrier on exactly that header, so a store into this object \
             would leave a real old->young edge unrecorded"
        );
    }
}

/// Perform an array-element store the way the guarded codegen sequence does:
/// the slot write is unconditional, and the barrier runs only when `gate`
/// accepts the PARENT. Returns whether the barrier was called.
unsafe fn gated_slot_store(
    parent: *mut crate::object::ObjectHeader,
    fields: *mut u64,
    child_bits: u64,
    gate: impl Fn(u8, u32) -> bool,
) -> bool {
    *fields = child_bits;
    let flags = (*header_from_user_ptr(parent as *const u8)).gc_flags;
    let active = crate::gc::PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT.load(Ordering::Relaxed);
    if gate(flags, active) {
        js_write_barrier_slot(ptr_bits(parent as usize), fields as u64, child_bits);
        return true;
    }
    false
}

/// **The stranding witness.** An OLD+TENURED parent, a YOUNG child, and the
/// exact store sequence codegen now emits.
///
/// 1. shipped gate, tenured parent — the barrier runs, the old→young edge
///    verifier is satisfied, and a remembered-set scan marks the child;
/// 2. SABOTAGED gate (always answers "no work" — the shape an inverted branch
///    or a wrong comparand produces) — the same store leaves the edge
///    unrecorded and the verifier REJECTS it: that child is stranded, and the
///    next minor frees it under a live reference;
/// 3. shipped gate, NURSERY parent — the barrier is skipped and the verifier is
///    still satisfied, which is the case the elision exists for and proves (2)
///    is about the parent's generation, not about skipping a barrier per se.
#[test]
fn sabotaged_parent_gate_strands_a_young_child_the_shipped_gate_keeps() {
    let _guard = GcTestIsolationGuard::new();

    // (1) shipped gate, OLD+TENURED parent. Old parent allocated FIRST: `young`
    // is a bare `usize` that production mode neither roots nor pins, so no
    // allocation may follow it.
    reset_remembered_set();
    clear_marks();
    let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
    let old_header = unsafe { header_from_user_ptr(old_obj as *const u8) };
    // `alloc_old_test_object` calls `arena_alloc_gc_old` directly and does NOT
    // set the bit — production old-gen objects always carry it (pinned by
    // `every_old_gen_birth_path_sets_tenured`), so set it here to model one.
    unsafe { (*old_header).gc_flags |= GC_FLAG_TENURED | GC_FLAG_MARKED };
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    assert!(
        unsafe {
            gated_slot_store(
                old_obj,
                fields,
                ptr_bits(young),
                codegen_parent_may_need_remembering,
            )
        },
        "a TENURED parent must take the barrier branch"
    );
    let stats = verify_old_to_young_edges_covered();
    assert_eq!(stats.checked_old_to_young_edges, 1);
    assert_eq!(stats.missing_edges, 0);
    let valid_ptrs = build_valid_pointer_set();
    let scan = mark_remembered_set_roots(&valid_ptrs);
    assert_eq!(scan.newly_marked, 1, "the child must survive the minor");
    unsafe { (*old_header).gc_flags &= !GC_FLAG_MARKED };
    clear_marks();
    remembered_set_clear();

    // (2) SABOTAGE: a gate that never accepts.
    fn never_needs_remembering(_flags: u8, _active: u32) -> bool {
        false
    }
    reset_remembered_set();
    clear_marks();
    let (old_obj2, fields2) = unsafe { alloc_old_test_object(1) };
    let old_header2 = unsafe { header_from_user_ptr(old_obj2 as *const u8) };
    unsafe { (*old_header2).gc_flags |= GC_FLAG_TENURED | GC_FLAG_MARKED };
    let young2 = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    assert!(
        !unsafe { gated_slot_store(old_obj2, fields2, ptr_bits(young2), never_needs_remembering) },
        "the sabotaged gate must skip the barrier — otherwise this arm proves nothing"
    );
    let rejected = std::panic::catch_unwind(verify_old_to_young_edges_covered);
    assert!(
        rejected.is_err(),
        "a skipped barrier on an OLD parent must leave the old->young edge unrecorded — this is \
         the stranded child the shipped gate must never produce"
    );
    unsafe { (*old_header2).gc_flags &= !GC_FLAG_MARKED };
    clear_marks();
    remembered_set_clear();

    // (3) shipped gate, NURSERY parent — skipped, and nothing is stranded.
    reset_remembered_set();
    clear_marks();
    let (young_parent, yfields) = unsafe { alloc_nursery_test_object(1) };
    let yheader = unsafe { header_from_user_ptr(young_parent as *const u8) };
    assert_eq!(
        unsafe { (*yheader).gc_flags } & GC_FLAG_TENURED,
        0,
        "a fresh nursery object must not be TENURED — otherwise this arm exercises nothing"
    );
    let young3 = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    assert!(
        !unsafe {
            gated_slot_store(
                young_parent,
                yfields,
                ptr_bits(young3),
                codegen_parent_may_need_remembering,
            )
        },
        "a nursery parent publishes no old->young edge, so the gate must skip the call"
    );
    let young_stats = verify_old_to_young_edges_covered();
    assert_eq!(
        young_stats.missing_edges, 0,
        "a young->young store strands nothing when its barrier is skipped"
    );
    clear_marks();
    remembered_set_clear();
}

/// **The incremental clause is load-bearing.** With a cycle active, a nursery
/// parent must still force the call, because the skipped work includes
/// `incremental_mark_barrier_value`'s insertion shading and that has nothing to
/// do with generations. Deleting the clause from the emitted predicate turns
/// this red.
#[test]
fn the_incremental_clause_forces_the_call_for_a_young_parent() {
    let nursery_parent_flags = GC_FLAG_ARENA;
    assert_eq!(
        nursery_parent_flags & GC_FLAG_TENURED,
        0,
        "the fixture must be a non-tenured parent for this test to mean anything"
    );
    assert!(
        !codegen_parent_may_need_remembering(nursery_parent_flags, 0),
        "with no cycle active a nursery parent is exactly the case the gate skips"
    );
    assert!(
        codegen_parent_may_need_remembering(nursery_parent_flags, 1),
        "with an incremental cycle active the SAME parent must take the call — otherwise the \
         store skips its insertion barrier and a live object is swept"
    );
}

// ---------------------------------------------------------------------------
// The RUNTIME twin of the same gate.
//
// Runtime-Rust construction paths (`js_regexp_new` and friends) call the
// barrier unconditionally, so a native header born in the nursery pays the
// full parent classification on every field it initialises while generated
// code, storing into the very same kind of object, skips it. These tests pin
// `gc::newborn_parent_needs_barrier` to the emitted predicate CLAUSE FOR
// CLAUSE, so the two can only drift by failing here.
// ---------------------------------------------------------------------------

/// Clause 1 (`GC_FLAG_TENURED`): the runtime twin must answer exactly what the
/// emitted gate answers for the same live header.
///
/// Sabotage that this catches: a twin that only consults the incremental
/// count answers "skip" for the tenured parent and fails the second assert —
/// which is the stranded-child bug of
/// `sabotaged_parent_gate_strands_a_young_child_the_shipped_gate_keeps`,
/// reached from the runtime side instead of the emitted side.
#[test]
fn the_runtime_twin_reads_the_tenured_clause_from_the_live_header() {
    let _guard = GcTestIsolationGuard::new();
    assert!(
        crate::gc::incremental_mark_barrier_globally_idle(),
        "this test isolates clause 1, so no cycle may be live"
    );

    let young = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT) as usize;
    assert_eq!(
        header_flags(young) & GC_FLAG_TENURED,
        0,
        "a fresh nursery allocation must not be TENURED — otherwise this test exercises nothing"
    );
    assert!(
        !unsafe { crate::gc::newborn_parent_needs_barrier(young) },
        "a nursery parent with no cycle live is exactly the case the gate exists to skip"
    );
    assert_eq!(
        unsafe { crate::gc::newborn_parent_needs_barrier(young) },
        codegen_parent_may_need_remembering(header_flags(young), 0),
        "the runtime twin and the emitted gate must agree on a nursery parent"
    );

    // The SAME address, now carrying the bit a promotion would have stamped.
    // Read live, so this models a header a collection promoted between its
    // allocation and the store that follows it.
    unsafe { (*header_from_user_ptr(young as *const u8)).gc_flags |= GC_FLAG_TENURED };
    assert!(
        unsafe { crate::gc::newborn_parent_needs_barrier(young) },
        "a TENURED parent owes the remembered set an entry and must take the full barrier"
    );
    assert_eq!(
        unsafe { crate::gc::newborn_parent_needs_barrier(young) },
        codegen_parent_may_need_remembering(header_flags(young), 0),
        "the runtime twin and the emitted gate must agree on a tenured parent"
    );
    unsafe { (*header_from_user_ptr(young as *const u8)).gc_flags &= !GC_FLAG_TENURED };
}

/// Clause 2 (the incremental count): with a cycle live anywhere, a nursery
/// parent must still take the call, because the skipped work includes the
/// SATB/insertion shading and that is not a generational question.
///
/// Sabotage that this catches: a twin that only reads the header's flags
/// answers "skip" while a cycle is marking, and a child linked in during that
/// window is never shaded.
#[test]
fn the_runtime_twin_forces_the_barrier_while_an_incremental_cycle_is_live() {
    let _guard = GcTestIsolationGuard::new();

    let young = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT) as usize;
    assert!(
        !unsafe { crate::gc::newborn_parent_needs_barrier(young) },
        "precondition: with no cycle live this parent is skipped"
    );

    crate::gc::PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT.fetch_add(1, Ordering::Relaxed);
    let forced = unsafe { crate::gc::newborn_parent_needs_barrier(young) };
    let codegen_answer = codegen_parent_may_need_remembering(
        header_flags(young),
        crate::gc::PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT.load(Ordering::Relaxed),
    );
    crate::gc::PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT.fetch_sub(1, Ordering::Relaxed);

    assert!(
        forced,
        "a live incremental cycle must force the call even for a nursery parent — dropping this \
         clause skips the insertion shading and sweeps a live child"
    );
    assert_eq!(
        forced, codegen_answer,
        "the runtime twin and the emitted gate must agree while a cycle is live"
    );
    assert!(
        !unsafe { crate::gc::newborn_parent_needs_barrier(young) },
        "the count is back to zero, so the same parent is skippable again"
    );
}

/// **Did this code run?** The gate is only worth anything if the real
/// `js_regexp_new` header reaches its skip arm — a fast path that is available
/// but never taken is the campaign's "measured flat" failure in advance.
///
/// Asserts on the header `js_regexp_new` actually produced, not on a synthetic
/// fixture: since #9845 it is a nursery allocation, so with no cycle live the
/// two `pattern_ptr` / `flags_ptr` stores skip the barrier entirely, and the
/// SAME header answers "take the barrier" the moment it carries the bit a
/// promotion would stamp.
#[cfg(feature = "regex-engine")]
#[test]
fn a_freshly_constructed_regexp_header_reaches_the_skip_arm() {
    let _guard = GcTestIsolationGuard::new();

    let pattern = crate::string::js_string_from_bytes(b"a(b)c".as_ptr(), 5);
    let flags = crate::string::js_string_from_bytes(b"g".as_ptr(), 1);
    let re = crate::regex::js_regexp_new(pattern, flags) as usize;

    assert_eq!(
        crate::arena::classify_heap_generation(re),
        crate::arena::HeapGeneration::Nursery,
        "#9845 allocates the RegExp header in the NURSERY; if that changes, the construction path \
         stops being the case this gate is written for"
    );
    assert!(
        !unsafe { crate::gc::newborn_parent_needs_barrier(re) },
        "the construction path must actually TAKE the skip arm — an available-but-unreached fast \
         path is indistinguishable from no fast path in a measurement"
    );

    unsafe { (*header_from_user_ptr(re as *const u8)).gc_flags |= GC_FLAG_TENURED };
    assert!(
        unsafe { crate::gc::newborn_parent_needs_barrier(re) },
        "the same header, promoted, must take the full barrier — this is the \
         `RegExp.prototype.compile`-on-a-tenured-receiver case"
    );
    unsafe { (*header_from_user_ptr(re as *const u8)).gc_flags &= !GC_FLAG_TENURED };
}
