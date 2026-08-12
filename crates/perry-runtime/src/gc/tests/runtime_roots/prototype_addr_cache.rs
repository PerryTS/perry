//! #6981 — the memoized `Array.prototype` / `Object.prototype` addresses are
//! raw pointers to MOVABLE objects, so they must survive relocation.
//!
//! `array::prototype_addr` memoizes both intrinsic addresses in process-global
//! `AtomicUsize` cells. `Array.prototype` relocates two ways, and both leave a
//! `GC_FLAG_FORWARDED` stub at the memoized address:
//!
//!   * `js_array_grow` — `Array.prototype[300] = v` reallocates the dense
//!     backing store and forwards the old head (no GC involved at all);
//!   * the copying young-gen minor — it evacuates the prototype and forwards.
//!
//! Every *reader* of an array pointer resolves it through `clean_arr_ptr`,
//! which follows forwarding chains. The cache did not. That mismatch is not a
//! cosmetic staleness: `array_oob_prototype_get`'s self-recursion guard is the
//! object-identity test `proto != receiver`, so once the two sides disagree
//! about the same object the guard stops firing and
//! `js_array_get_f64` ⇄ `array_oob_prototype_get` recurse without bound until
//! the thread's stack guard page (`SIGSEGV`, "excessive recursion").
//!
//! Two independent defences, one test each:
//!
//!   1. `memoized_prototype_addr` heals the cell through the forwarding chain.
//!      This is what covers `js_array_grow`, which the collector never sees.
//!   2. `scan_prototype_addr_cache_roots_mut` is a registered mutable root
//!      scanner, so a relocating cycle REWRITES the cell. Healing alone is not
//!      enough here: once the from-space stub is swept and its block recycled
//!      the forwarded bit is gone, and the cache would name an unrelated live
//!      object.
//!
//! # Why these run on private cells (#7955)
//!
//! Both defences used to be driven by planting a synthetic stub in the SHIPPED
//! `static`s and reading it back. That made every assertion here depend on no
//! other libtest thread touching the realm's real intrinsics in between — and
//! two things routinely do: `array_prototype_addr()` / `object_prototype_addr()`
//! HEAL the cell in place, and any collection's registered
//! `scan_prototype_addr_cache_roots_mut` REWRITES it. Either overwrites the
//! plant, and the test reports a stale-cache failure that says nothing about
//! the code under test. The save/restore guard made it worse rather than
//! better: restoring the value read at test entry stamps a stale address over
//! whatever another thread resolved meanwhile.
//!
//! Both defences are algebra over an `&AtomicUsize`, so each case now owns its
//! cell and the shipped `static`s are never written from a test. What that
//! decomposition would otherwise lose — "the collector rewrites every cell an
//! accessor reads" — is not recovered by a test at all but by CONSTRUCTION:
//! `PROTOTYPE_ADDR_CACHES` is one table, the scanner iterates it and the
//! accessors index it. `the_shipped_cells_are_the_ones_the_scanner_visits`
//! pins the table itself, read-only, so it cannot be raced either.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A memoized-prototype-address cell owned by ONE test.
///
/// `memoized_prototype_addr` / `rewrite_prototype_addr_slot` take the cell as
/// an argument, so the #6981 algebra is exercised exactly as shipped without
/// any test writing to the realm's real intrinsic cells.
fn private_cache_cell() -> AtomicUsize {
    AtomicUsize::new(usize::MAX)
}

/// Allocate a nursery object to stand in for the intrinsic.
///
/// Not exposed on its own: nothing is live in this function before the
/// allocation, so there is no raw pointer for an in-flight trigger to move
/// out from under. Callers that keep the returned pointer live across a
/// FURTHER allocation (`forwarded_pair`, the multi-hop test, the collector
/// rewrite test) carry their own guard.
fn nursery_stand_in() -> *mut u8 {
    crate::arena::arena_alloc_gc(64, 8, GC_TYPE_ARRAY)
}

/// Evacuate `from`: allocate an old-gen destination and forward `from` → `to`.
///
/// `from` is a raw pointer to a movable nursery object that stays live across
/// the allocation below. If that allocation lands on the block-full slow
/// path, `arena_cell_alloc` calls `gc_check_trigger()`, and — absent
/// suppression — an automatic collection could relocate/free `from` before
/// the forwarding address is installed, corrupting the synthetic setup this
/// test file relies on (#6981's tests never intend to exercise a *real*
/// concurrent collection here, only the hand-driven forwarding chain).
fn evacuate(from: *mut u8) -> usize {
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let to = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_ARRAY);
    unsafe {
        set_forwarding_address(header_from_user_ptr(from) as *mut GcHeader, to);
    }
    to as usize
}

/// Allocate `from` and `to`, forward `from` → `to`, and return the pair.
///
/// `from` is also live in THIS frame across the call to `evacuate` (which
/// performs the allocation). Guarded here too, in addition to `evacuate`'s
/// own guard, so the suppression holds for as long as `from` is live in any
/// frame on this call chain, independent of `evacuate`'s internals.
fn forwarded_pair() -> (usize, usize) {
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let from = nursery_stand_in();
    let to = evacuate(from);
    (from as usize, to)
}

/// DEFENCE 1. A memoized address that has been forwarded — by `js_array_grow`
/// or by an evacuating cycle — must read back as the object's CURRENT address,
/// and the cell must be healed in place so the next reader pays nothing.
///
/// Without the heal this returns the from-space stub, which is a different
/// address for the same object than every `clean_arr_ptr`-resolved receiver —
/// the exact condition that defeats `array_oob_prototype_get`'s
/// self-recursion guard.
#[test]
fn prototype_addr_reads_through_a_forwarding_stub() {
    // Not exposed at this level: `from`/`to` come back as plain `usize`s from
    // `forwarded_pair`, which carries its own trigger guard, and nothing else
    // in this loop body allocates.
    for which in ["array", "object"] {
        let cell = private_cache_cell();
        let (from, to) = forwarded_pair();
        cell.store(from, Ordering::Relaxed);

        assert_eq!(
            crate::array::test_memoized_prototype_addr(&cell),
            Some(to),
            "the {which} prototype cell must resolve the GC forwarding chain: a \
             stale from-space address is a DIFFERENT address for the SAME \
             object than every clean_arr_ptr-resolved receiver, which defeats \
             the `proto != receiver` self-recursion guard in the hole/OOB read \
             fallback and hangs the mutator (#6981)"
        );
        assert_eq!(
            cell.load(Ordering::Relaxed),
            to,
            "the read must write the healed address back so the hot path stays \
             a single relaxed load"
        );
    }
}

/// A cell that has never resolved reports "not resolved" rather than healing
/// the sentinel — that is what sends the accessor to the `globalThis`
/// bootstrap instead of pinning a bogus prototype.
#[test]
fn an_unresolved_prototype_cell_reports_no_address() {
    let cell = private_cache_cell();
    assert_eq!(crate::array::test_memoized_prototype_addr(&cell), None);
    assert_eq!(cell.load(Ordering::Relaxed), usize::MAX);
}

/// Multi-hop chains (grow, then grow again, then evacuate) must resolve all the
/// way to the live head.
#[test]
fn prototype_addr_reads_through_a_multi_hop_forwarding_chain() {
    // `first` is live across `second`'s allocation, and both `first` and
    // `second` are live across `final_user`'s allocation — any of the three
    // could reach the block-full slow path's `gc_check_trigger()`.
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let first = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_ARRAY);
    let second = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_ARRAY);
    let final_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_ARRAY);
    unsafe {
        set_forwarding_address(header_from_user_ptr(first) as *mut GcHeader, second);
        set_forwarding_address(header_from_user_ptr(second) as *mut GcHeader, final_user);
    }

    let cell = private_cache_cell();
    cell.store(first as usize, Ordering::Relaxed);
    assert_eq!(
        crate::array::test_memoized_prototype_addr(&cell),
        Some(final_user as usize),
        "every forwarding hop must be followed (#6981)"
    );
}

/// DEFENCE 2. The collector must REWRITE the cell, not merely leave it
/// resolvable — from-space is reset and handed back to the mutator at the end
/// of the cycle, after which the forwarded bit is gone and healing cannot
/// recover the address.
#[test]
fn prototype_addr_cache_is_rewritten_by_the_collector() {
    // `array_from` is live across the second `nursery_stand_in` call below
    // (its own allocation, unguarded on its own), and both from-pointers stay
    // live across the `evacuate` calls that follow.
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    // The from-space objects must exist before the valid-pointer set is built —
    // that set is what tells the rewrite visitor an address is a real heap
    // object, exactly as in a real cycle.
    let array_from = nursery_stand_in();
    let object_from = nursery_stand_in();
    let valid_ptrs = build_valid_pointer_set();
    let array_to = evacuate(array_from);
    let object_to = evacuate(object_from);
    let array_cell = private_cache_cell();
    let object_cell = private_cache_cell();
    array_cell.store(array_from as usize, Ordering::Relaxed);
    object_cell.store(object_from as usize, Ordering::Relaxed);

    for cell in [&array_cell, &object_cell] {
        crate::array::test_rewrite_prototype_addr_slot(
            cell,
            &mut RuntimeRootVisitor::for_rewrite(&valid_ptrs),
        );
    }

    assert_eq!(
        array_cell.load(Ordering::Relaxed),
        array_to,
        "a memoized prototype cell must be rewritten by the relocating cycle — \
         it is a raw address of a movable object, exactly like the other \
         registered side tables (#6981)"
    );
    assert_eq!(
        object_cell.load(Ordering::Relaxed),
        object_to,
        "the rewrite is per-cell, so both rows of PROTOTYPE_ADDR_CACHES get it \
         (#6981)"
    );
}

/// …and it must actually be REGISTERED. The scanner above can be invoked
/// directly from a test whether or not `gc_init` ever mentions it, so assert
/// the wiring separately: an unregistered scanner is a no-op in production.
#[test]
fn prototype_addr_cache_scanner_is_registered() {
    crate::gc::gc_init();
    let registered = crate::gc::roots::MUTABLE_ROOT_SCANNERS.with(|scanners| {
        scanners.borrow().iter().any(|entry| {
            entry.scanner as usize
                == crate::array::scan_prototype_addr_cache_roots_mut as MutableRootScanner as usize
        })
    });
    assert!(
        registered,
        "scan_prototype_addr_cache_roots_mut must be registered in gc_init — \
         otherwise the collector never rewrites ARRAY_PROTO_ADDR and the cache \
         is left naming from-space after the block is recycled (#6981)"
    );
}

/// The not-yet-computed sentinel is not a heap address and must be left alone —
/// a scanner that rewrote it would pin a bogus prototype for the whole process.
#[test]
fn prototype_addr_cache_scanner_leaves_the_unset_sentinel_alone() {
    let valid_ptrs = build_valid_pointer_set();
    let cell = private_cache_cell();

    crate::array::test_rewrite_prototype_addr_slot(
        &cell,
        &mut RuntimeRootVisitor::for_rewrite(&valid_ptrs),
    );

    assert_eq!(cell.load(Ordering::Relaxed), usize::MAX);
}

/// The WIRING, and deliberately read-only so it cannot be raced (#7955).
///
/// The cases above prove the algebra on cells they own; on its own that would
/// leave nothing asserting that the shipped `static`s are the cells in play —
/// the "gate runs but its subject never did" shape. `PROTOTYPE_ADDR_CACHES` is
/// the single table the scanner iterates and the accessors index, so this
/// pins the table: two DISTINCT cells (a copy-pasted row would give
/// `Array.prototype`'s address to `object_prototype_addr()` and leave one cell
/// unrewritten), each paired with the `globalThis` builtin whose `.prototype`
/// its accessor resolves. Nothing here writes.
#[test]
fn the_shipped_cells_are_the_ones_the_scanner_visits() {
    let wiring = crate::array::test_prototype_addr_cache_wiring();
    assert_eq!(
        wiring[0].1, b"Array",
        "row 0 is what array_prototype_addr() indexes; it must bootstrap from \
         globalThis.Array"
    );
    assert_eq!(
        wiring[1].1, b"Object",
        "row 1 is what object_prototype_addr() indexes; it must bootstrap from \
         globalThis.Object"
    );
    assert!(
        !std::ptr::eq(wiring[0].0, wiring[1].0),
        "the two intrinsics must memoize into DIFFERENT cells — sharing one \
         cell makes the second accessor return the first intrinsic's address \
         and leaves the collector with nothing to rewrite for it (#6981)"
    );
}
