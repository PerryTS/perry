//! `GC_FLAG_PINNED` custody, and the young-pin latch the copying minor's
//! eligibility preflight is skipped on (#7645).
//!
//! # Why this module exists
//!
//! The copying minor traverses the young object graph **twice**: once in
//! `CopiedMinorEligibility::evaluate`'s preflight, to prove nothing reachable
//! is pinned, and again to copy. On `json_pipeline` the first traversal is
//! ~22% of the hot phase and produces no collection result at all.
//!
//! The preflight walk (`CopyingNurseryPreflight::drain`) answers exactly two
//! questions:
//!
//! 1. Is any transitively reachable `Eden`/`FromSurvivor` object
//!    `GC_FLAG_PINNED`? (`check_ptr_with_reason`)
//! 2. Was a non-arena candidate seen while the malloc registry was
//!    unavailable and non-empty at cycle start?
//!    (`classify_for_preflight`)
//!
//! (2) is already decidable in O(1) from `CopyingPointerSet`'s two fields.
//! (1) is O(live young graph) — but only because it *searches* for a fact that
//! can instead be *recorded at the moment it is created*. That is what this
//! module does: every write of `GC_FLAG_PINNED` goes through [`pin_object`],
//! which arms a process-wide monotone latch when (and only when) the pinned
//! object is in a space the copying minor would relocate.
//!
//! When the latch is clear, no object anywhere carries a young pin, so the
//! walk provably returns `None` and skipping it is observationally equivalent
//! (modulo the layout/malloc-lookup telemetry counters the walk incremented).
//! Note the direction: "no young pinned object exists at all" is *stronger*
//! than the walk's "no young pinned object is reachable", so the substitution
//! is conservative, not merely equal.
//!
//! # The safety argument, and what enforces it
//!
//! Skipping this guard is a use-after-move if it is ever wrong: `move_young`
//! relocates a pinned object exactly as it would any other (it only *preserves*
//! the bit, `copying.rs`), and the raw `usize` in `PENDING_THREAD_RESULTS` has
//! no scanner to rewrite. So the latch's completeness is load-bearing and is
//! enforced three ways, not asserted in prose:
//!
//! * **Statically, at every write site.** `scripts/gc_pin_sites.py` (run in
//!   `lint`) fails on any source line that sets the pinned bit outside
//!   [`pin_object`], and equally on an allowlist entry that no longer matches
//!   anything. It deliberately matches both the named-constant form
//!   (`gc_flags |= GC_FLAG_PINNED`) and the raw-byte form
//!   (`*gc_flags_ptr |= 0x04`) — two of the six pin sites that existed when
//!   this landed used the raw byte and are invisible to a
//!   `grep GC_FLAG_PINNED`.
//! * **Dynamically, at the moment it would matter.** `move_young` checks the
//!   pinned bit on the flags byte it has already loaded, and aborts if a
//!   *preflight-skipped* cycle is about to relocate a pinned object. That is
//!   the precise instant an incomplete latch becomes memory corruption, and it
//!   costs one `and` plus a never-taken branch.
//! * **In tests.** The copying suite's pinned-fallback tests plant their pins
//!   through [`pin_object`], so deleting the arming below turns them red
//!   rather than leaving them green on an unsound configuration.
//!
//! # Why the latch is monotone
//!
//! A decrementing counter would recover the fast path after a transient pin
//! (a settled `fetch` promise, say). It was rejected for this change because
//! it adds a *second* completeness obligation of the same severity: every
//! unpin site must also be complete, and a spurious or double decrement is
//! silently unsound in exactly the same use-after-move way. Monotone needs one
//! proof. A process that has ever pinned young pays the walk forever, which is
//! the conservative direction.
//!
//! Concretely, the pin sites that arm the latch in production are the
//! Eden-resident ones — `js_promise_new()` promises pinned for native
//! resolution (`perry-stdlib`'s `async_bridge`, i.e. fetch/zlib/ws/bcrypt),
//! `Atomics.waitAsync`, and the AppKit text reads. Programs that use them get
//! today's behaviour; compute- and JSON-shaped programs get the walk removed.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::types::{GcHeader, GC_FLAG_ARENA, GC_FLAG_PINNED};

/// Has any object in a space the copying minor relocates ever been pinned?
///
/// Monotone: set by [`pin_object`], never cleared outside tests. Cleared only
/// through [`test_reset_young_pin_latch`], which the copying-nursery test
/// isolation guard calls while holding the suite's global lock.
static YOUNG_PIN_EVER: AtomicBool = AtomicBool::new(false);

/// Copying minors that skipped both preflight walks. The live-subject counter
/// for any "the preflight is gone" verdict — a benchmark or gate that reports
/// a win without this being non-zero measured nothing (#7024/#7025).
static PREFLIGHT_SKIPS: AtomicU64 = AtomicU64::new(0);

/// Copying minors that ran the preflight walks.
static PREFLIGHT_WALKS: AtomicU64 = AtomicU64::new(0);

/// Set `GC_FLAG_PINNED` on `header`, arming the young-pin latch if this pin
/// constrains the copying minor.
///
/// **This is the only sanctioned way to set the bit.** See the module docs for
/// what rests on that and what enforces it.
///
/// # Safety
///
/// `header` must point at a live `GcHeader` (i.e. `user_ptr - GC_HEADER_SIZE`
/// of a live allocation).
#[inline]
pub unsafe fn pin_object(header: *mut GcHeader) {
    if header.is_null() {
        return;
    }
    if pin_constrains_copying_minor(header) {
        // Release so a collector on another thread that observes the latch
        // also observes the flag write below it in program order.
        YOUNG_PIN_EVER.store(true, Ordering::Release);
    }
    (*header).gc_flags |= GC_FLAG_PINNED;
}

/// Clear `GC_FLAG_PINNED` on `header`. Does **not** disarm the latch — see the
/// module docs on why the latch is monotone.
///
/// # Safety
///
/// As [`pin_object`].
#[inline]
pub unsafe fn unpin_object(header: *mut GcHeader) {
    if header.is_null() {
        return;
    }
    (*header).gc_flags &= !GC_FLAG_PINNED;
}

/// Would a pin on `header` be able to force `CopiedMinorFallbackReason::
/// PinnedYoung*`?
///
/// `CopyingNurseryPreflight::check_ptr_with_reason` trips only on
/// `CopyingPointerKind::Eden` / `FromSurvivor`, and `CopyingPointerSet::
/// classify_arena` reaches those kinds only for an address whose header sits
/// in `NurseryEden`/`Survivor0`/`Survivor1` of *this thread's* arena. So:
///
/// * A malloc-space object (no `GC_FLAG_ARENA`) is never `Eden`/`FromSurvivor`
///   and is never relocated by a copying minor. It cannot arm the latch —
///   which is what keeps `spawn`'s deliberately malloc-resident cross-thread
///   promise (`thread.rs`) from costing every later cycle a walk.
/// * `Longlived` and `Old` are likewise never relocated by a copying minor,
///   which is why the `SMALL_INT_CACHE` pins (`string/format.rs`, allocated
///   through `js_string_from_bytes_longlived`) are free.
/// * Anything else — the nursery spaces, and `Unknown`, which is what another
///   agent's arena classifies as from here — arms it.
///
/// Spaces never flow backwards (nothing in `Longlived`/`Old` re-enters the
/// nursery), so a decision taken at pin time stays valid for as long as the
/// pin does.
///
/// # Safety
///
/// As [`pin_object`].
#[inline]
unsafe fn pin_constrains_copying_minor(header: *mut GcHeader) -> bool {
    if (*header).gc_flags & GC_FLAG_ARENA == 0 {
        return false;
    }
    !matches!(
        crate::arena::classify_heap_space(header as usize),
        crate::arena::HeapSpace::Longlived | crate::arena::HeapSpace::Old
    )
}

/// Has a young pin ever been created? While this is false the eligibility
/// preflight's pin question is answered.
#[inline]
pub(super) fn young_pin_latch_armed() -> bool {
    YOUNG_PIN_EVER.load(Ordering::Acquire)
}

#[inline]
pub(super) fn note_preflight_skipped() {
    PREFLIGHT_SKIPS.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub(super) fn note_preflight_walked() {
    PREFLIGHT_WALKS.fetch_add(1, Ordering::Relaxed);
}

/// Copying minors that skipped both eligibility preflight walks.
pub fn copied_minor_preflight_skips() -> u64 {
    PREFLIGHT_SKIPS.load(Ordering::Relaxed)
}

/// Copying minors that ran the eligibility preflight walks.
pub fn copied_minor_preflight_walks() -> u64 {
    PREFLIGHT_WALKS.load(Ordering::Relaxed)
}

/// Clear the latch so a test starts from a known state. Callers must hold the
/// copying-nursery isolation lock; `reset_copying_nursery_runtime_test_state`
/// does.
#[cfg(test)]
pub(crate) fn test_reset_young_pin_latch() {
    YOUNG_PIN_EVER.store(false, Ordering::Release);
}

/// `extern "C"` form of [`pin_object`] taking the **user** pointer, for crates
/// that reach the runtime through FFI declarations rather than a Rust
/// dependency edge (`perry-ui-macos`, which used to open-code
/// `*(ptr - 8 + 1) |= 0x04`).
///
/// # Safety
///
/// `user_ptr` must be a live allocation preceded by an 8-byte `GcHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_gc_pin_user_ptr(user_ptr: *mut u8) {
    if user_ptr.is_null() {
        return;
    }
    pin_object(user_ptr.sub(super::types::GC_HEADER_SIZE) as *mut GcHeader);
}
