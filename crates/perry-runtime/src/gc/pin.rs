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

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::types::{GcHeader, GC_FLAG_ARENA, GC_FLAG_PINNED};

crate::perry_thread_local! {
    static COPYING_WALK_PHASE: Cell<Option<&'static str>> =
        const { Cell::new(None) };
}

/// RAII label for the walk that is about to call into `move_young`.
///
/// The pin-latch abort used to print only the garbage header. On #7803/#7990
/// that header is *incoherent* (INTERNED on a Map, a 2 GiB nursery size), so
/// the interesting fact is which walk followed the stale slot. Set around
/// each mark/rewrite walk in `copying.rs`; read by
/// [`pinned_young_move_report`].
pub(super) struct CopyingWalkPhaseGuard {
    prev: Option<&'static str>,
}

impl CopyingWalkPhaseGuard {
    pub(super) fn enter(name: &'static str) -> Self {
        let prev = COPYING_WALK_PHASE.with(|c| c.replace(Some(name)));
        Self { prev }
    }
}

impl Drop for CopyingWalkPhaseGuard {
    fn drop(&mut self) {
        COPYING_WALK_PHASE.with(|c| c.set(self.prev));
    }
}

fn copying_walk_phase() -> Option<&'static str> {
    COPYING_WALK_PHASE.with(|c| c.get())
}

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

/// Set `GC_FLAG_PINNED` on an object the CALLER has already proven cannot be
/// young-arena resident, without consulting the space classifier.
///
/// # Why this exists, and why it is not merely an optimisation
///
/// [`pin_object`] reaches `crate::arena::classify_heap_space`, and that edge is
/// load-bearing for a reason that has nothing to do with the GC: the
/// `perry-ext-*` crates link a **feature-stripped** runtime through
/// `perry-ffi`'s `runtime-link` and are built with `-Wl,-dead_strip`.
/// Introducing this call from `thread.rs` / `string/format.rs` kept a reference
/// chain alive that the stripper had previously removed, and five ext crates
/// stopped linking with `Undefined symbols: _js_blob_new,
/// _js_fetch_with_options, _js_fetch_notify_signal_aborted` (#7650, bisected to
/// that commit against a clean parent). `cargo-test` scopes per-PR runs to the
/// changed crates' reverse-dependency closure and the FULL workspace is
/// tag/nightly-only, so no per-PR gate could have seen it.
///
/// Making [`pin_object`] conservative instead — arming the latch for any
/// `GC_FLAG_ARENA` object — would also remove the edge, but it would arm on
/// exactly the long-lived pins this variant serves (`format.rs` pins long-lived
/// strings), throwing away the preflight skip #7645 bought.
///
/// # Safety
///
/// As [`pin_object`], **plus** the caller must guarantee `header` is malloc
/// space, `Longlived`, or `Old`. Pinning a young-arena object through here
/// leaves the latch disarmed, and a copying minor will then relocate a pinned
/// object — memory corruption, not a missed optimisation. `debug_assert` catches
/// it in test builds, and the claim is checked for every real call site by
/// `pin_object_non_young_call_sites_are_never_young` in
/// `gc/tests/copying/latch.rs`; **add a case there when you add a caller.**
#[inline]
pub unsafe fn pin_object_non_young(header: *mut GcHeader) {
    if header.is_null() {
        return;
    }
    debug_assert!(
        !pin_constrains_copying_minor(header),
        "pin_object_non_young called on a young-arena object: the young-pin \
         latch stays disarmed and the copying minor will relocate it"
    );
    (*header).gc_flags |= GC_FLAG_PINNED;
}

/// Test accessor for the young-pin predicate, so
/// `pin_object_non_young_call_sites_are_never_young` can assert the invariant
/// its callers rest on without duplicating the classification logic.
#[cfg(test)]
pub(crate) unsafe fn pin_constrains_copying_minor_for_tests(header: *mut GcHeader) -> bool {
    pin_constrains_copying_minor(header)
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

/// Is this header self-consistent, given what each flag is allowed to mean?
///
/// Returns `None` when nothing contradicts, or `Some(reason)` when the header
/// cannot describe a live object of the type it claims. Used by the
/// `move_young` pin-latch abort: the flags and type it already has in hand are
/// the *only* evidence that exists at the instant of the fault, and they
/// separate "the young-pin latch is incomplete" from "the copier followed a
/// dangling pointer into recycled memory" — two faults with completely
/// different investigations that the abort used to report identically.
pub(super) fn header_incoherence(obj_type: u8, size: u32, flags: u8) -> Option<String> {
    use super::types::{GC_FLAG_INTERNED, GC_HEADER_SIZE, GC_TYPE_MAX, GC_TYPE_STRING};
    if obj_type == 0 || obj_type > GC_TYPE_MAX {
        return Some(format!(
            "obj_type={obj_type} is outside the defined range 1..={GC_TYPE_MAX}"
        ));
    }
    if flags & GC_FLAG_INTERNED != 0 && obj_type != GC_TYPE_STRING {
        return Some(format!(
            "GC_FLAG_INTERNED is set, but it is written in exactly one place \
             (string/intern.rs) and only on GC_TYPE_STRING — never on {}",
            gc_type_label(obj_type)
        ));
    }
    let total = size as usize;
    if total < GC_HEADER_SIZE || total > super::copying::MAX_YOUNG_MOVE_BYTES {
        return Some(format!(
            "size={total} is outside the range a nursery-resident object can \
             have ({GC_HEADER_SIZE}..={})",
            super::copying::MAX_YOUNG_MOVE_BYTES
        ));
    }
    None
}

/// The `move_young` pin-latch abort's body: what happened, what the header
/// says about which fault this is, and the candidates in the order the
/// evidence separates them.
///
/// # Why this is not one sentence naming one cause
///
/// It was, and the cause it named was wrong. #7645's original text asserted
/// "some site sets `GC_FLAG_PINNED` without going through `gc::pin_object`"
/// and told the reader to run `scripts/gc_pin_sites.py`. On the tree where the
/// abort was next observed (#7990, zod dep-corpus, ~1 run in 16) that tool
/// reports **OK** — every pin does originate in `pin_object`, and both of its
/// allowlisted exceptions are test-only and unreachable from a user program.
/// So the message sent every reader at a hypothesis its own tool refutes,
/// which costs more than saying nothing.
pub(super) fn pinned_young_move_report(
    header_addr: usize,
    obj_type: u8,
    size: u32,
    flags: u8,
) -> String {
    let mut out = format!(
        "[gc-pin-latch] FATAL: copying minor is about to relocate a PINNED young \
         object on a preflight-skipped cycle. header={header_addr:#x} \
         obj_type={obj_type} ({}) size={size} flags={flags:#04x} ({})\n",
        gc_type_label(obj_type),
        flag_names(flags),
    );
    match header_incoherence(obj_type, size, flags) {
        Some(reason) => {
            out.push_str(&format!(
                "  header coherence: INCONSISTENT — {reason}.\n  \
                 So this is probably NOT a live pinned object and the young-pin latch \
                 is probably innocent: the copier reached a header in memory that used \
                 to hold something else, i.e. a slot that was not rooted across a \
                 collection (#7154 class). Chase THAT first:\n    \
                 PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1 \
                 PERRY_GC_PROTECT_FROMSPACE_DEPTH=800\n  \
                 (the default depth of 4 quarantines four retired page-sets; a value \
                 can cross hundreds of collections between its last valid observation \
                 and its stale use, and then the default misses it silently.)\n",
            ));
        }
        None => {
            out.push_str(
                "  header coherence: consistent — the flags and type do not contradict, \
                 so this reads as a real pinned object in a space the copying minor \
                 relocates.\n",
            );
        }
    }
    out.push_str(&format!(
        "  copying walk phase: {}\n",
        copying_walk_phase().unwrap_or("(unset — not inside a named walk)")
    ));
    if flags & super::types::GC_FLAG_TENURED != 0 {
        out.push_str(
            "  note: GC_FLAG_TENURED next to a young space is NOT an anomaly. The \
             non-moving generational path tenures in place — a tenured object stays \
             physically in the nursery and the trace merely pretends it is old \
             (gc/types.rs, GC_FLAG_TENURED).\n",
        );
    }
    out.push_str(
        "  candidates, in the order the evidence above separates them:\n   \
         1. a slot that was not rooted across a collection point handed the copier a \
            stale header (see the coherence verdict; docs/src/internals/gc-rooting-invariant.md).\n   \
         2. `pin_object_non_young` was called on a young-arena object. Its debug_assert \
            is compiled out of release builds, and only the `pin_object_non_young_\
            call_sites_are_never_young` test checks the callers — a caller added \
            without a case there is invisible.\n   \
         3. `pin_object` classified the object Longlived/Old at pin time and it is young \
            now. `gc/pin.rs` rests on spaces never flowing backwards.\n   \
         4. the preflight-skip decision (`preflight_walks_decided`, gc/copying.rs) is \
            wrong even though the latch is complete.\n   \
         5. LAST: a pin site outside `gc::pin_object`. `python3 scripts/gc_pin_sites.py` \
            decides this one, and on a clean tree it answers OK — which is why #7645's \
            original text naming it as THE cause was misleading (#7990).",
    );
    out
}

/// Human-readable name for a `GcHeader::obj_type`.
///
/// `types::gc_type_name` is `#[cfg(feature = "diagnostics")]`, and this abort
/// has to print the same text in every build — a fault report that degrades
/// with the feature set is a fault report nobody can compare against.
fn gc_type_label(obj_type: u8) -> &'static str {
    super::types::gc_type_info(obj_type).map_or("unknown", |info| info.name)
}

/// Render `gc_flags` as the constant names it is made of, so a reader does not
/// have to decode a hex byte against `gc/types.rs` by hand.
fn flag_names(flags: u8) -> String {
    use super::types::{
        GC_FLAG_ARENA, GC_FLAG_FORWARDED, GC_FLAG_HAS_SURVIVED, GC_FLAG_INTERNED, GC_FLAG_MARKED,
        GC_FLAG_PINNED, GC_FLAG_SHAPE_SHARED, GC_FLAG_TENURED,
    };
    let mut parts: Vec<&str> = Vec::new();
    for (bit, name) in [
        (GC_FLAG_MARKED, "MARKED"),
        (GC_FLAG_ARENA, "ARENA"),
        (GC_FLAG_PINNED, "PINNED"),
        (GC_FLAG_SHAPE_SHARED, "SHAPE_SHARED"),
        (GC_FLAG_INTERNED, "INTERNED"),
        (GC_FLAG_TENURED, "TENURED"),
        (GC_FLAG_HAS_SURVIVED, "HAS_SURVIVED"),
        (GC_FLAG_FORWARDED, "FORWARDED"),
    ] {
        if flags & bit != 0 {
            parts.push(name);
        }
    }
    if parts.is_empty() {
        "no flags".to_string()
    } else {
        parts.join("|")
    }
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

#[cfg(test)]
mod report_tests {
    use super::*;
    use crate::gc::types::{
        GC_FLAG_ARENA, GC_FLAG_INTERNED, GC_FLAG_MARKED, GC_FLAG_PINNED, GC_FLAG_TENURED,
        GC_TYPE_MAP, GC_TYPE_STRING,
    };

    /// The exact header #7990 reported, byte for byte:
    /// `obj_type=8 size=731 flags=0x37`. The old message called this "a pinned
    /// young Map" and sent the reader to `gc_pin_sites.py`, which answers OK.
    /// The header itself says it is not a coherent Map at all.
    #[test]
    fn the_7990_header_is_reported_as_incoherent() {
        let flags =
            GC_FLAG_MARKED | GC_FLAG_ARENA | GC_FLAG_PINNED | GC_FLAG_INTERNED | GC_FLAG_TENURED;
        assert_eq!(flags, 0x37, "the issue's flags byte");
        let reason = header_incoherence(GC_TYPE_MAP, 731, flags)
            .expect("INTERNED on a Map contradicts string/intern.rs");
        assert!(reason.contains("GC_FLAG_INTERNED"), "{reason}");

        let report = pinned_young_move_report(0x2db2f681350, GC_TYPE_MAP, 731, flags);
        assert!(report.contains("INCONSISTENT"), "{report}");
        assert!(
            report.contains("PERRY_GC_PROTECT_FROMSPACE_DEPTH=800"),
            "{report}"
        );
        // The decoded flag names, so a reader never hand-decodes 0x37 again.
        assert!(
            report.contains("MARKED|ARENA|PINNED|INTERNED|TENURED"),
            "{report}"
        );
    }

    /// A coherent header must NOT be described as a stale pointer — the
    /// verdict has to be able to come out both ways or it is decoration.
    #[test]
    fn a_coherent_pinned_young_header_is_not_blamed_on_a_stale_pointer() {
        let flags = GC_FLAG_MARKED | GC_FLAG_ARENA | GC_FLAG_PINNED;
        assert!(header_incoherence(GC_TYPE_MAP, 64, flags).is_none());
        let report = pinned_young_move_report(0x1000, GC_TYPE_MAP, 64, flags);
        assert!(report.contains("header coherence: consistent"), "{report}");
        assert!(!report.contains("INCONSISTENT"), "{report}");
    }

    #[test]
    fn interned_on_a_string_is_coherent() {
        let flags = GC_FLAG_MARKED | GC_FLAG_ARENA | GC_FLAG_PINNED | GC_FLAG_INTERNED;
        assert!(header_incoherence(GC_TYPE_STRING, 48, flags).is_none());
    }

    #[test]
    fn out_of_range_type_and_size_are_caught() {
        let flags = GC_FLAG_MARKED | GC_FLAG_ARENA | GC_FLAG_PINNED;
        assert!(header_incoherence(200, 64, flags).is_some());
        assert!(header_incoherence(0, 64, flags).is_some());
        // Smaller than a header, and larger than any nursery object.
        assert!(header_incoherence(GC_TYPE_MAP, 2, flags).is_some());
        assert!(header_incoherence(GC_TYPE_MAP, (1 << 20) + 1, flags).is_some());
    }

    /// The refuted hypothesis must not lead. #7645's text named the pin-site
    /// scan as THE cause; it is now candidate 5 of 5, and the message says why.
    #[test]
    fn the_pin_site_scan_is_the_last_candidate_not_the_first() {
        let flags = GC_FLAG_MARKED | GC_FLAG_ARENA | GC_FLAG_PINNED;
        let report = pinned_young_move_report(0x1000, GC_TYPE_MAP, 64, flags);
        let scan = report.find("gc_pin_sites.py").expect("names the tool");
        let rooting = report
            .find("not rooted across a collection point")
            .expect("names the rooting candidate");
        assert!(
            rooting < scan,
            "the rooting candidate must precede the pin-site scan:\n{report}"
        );
        assert!(report.contains("5. LAST"), "{report}");
    }

    /// TENURED on a young object is normal, not evidence. The message has to
    /// say so, because the issue flagged it as suspicious.
    #[test]
    fn tenured_on_a_young_object_is_explained_rather_than_flagged() {
        let flags = GC_FLAG_MARKED | GC_FLAG_ARENA | GC_FLAG_PINNED | GC_FLAG_TENURED;
        let report = pinned_young_move_report(0x1000, GC_TYPE_MAP, 64, flags);
        assert!(report.contains("NOT an anomaly"), "{report}");
        assert!(report.contains("tenures in place"), "{report}");
    }
}
