//! The loop back-edge poll's arming word — what `js_gc_loop_safepoint` costs
//! when there is nothing to do.
//!
//! # Why this exists
//!
//! `js_gc_loop_safepoint` is called at EVERY allocating loop back-edge. In
//! `gc-handoff/bench/churn_alloc.ts` that is 20 million calls, in
//! `churn_alloc_big.ts` 200 million. So what the poll costs on the path where
//! nothing is pending is not a GC cost at all — it is a per-iteration tax on
//! every allocating loop in the language, and it is paid whether or not a
//! collection ever happens.
//!
//! Until this module existed, that path was:
//!
//!   1. `gc_moving_loop_polls_enabled()` — an acquire load on a `OnceLock`,
//!   2. `note_loop_poll_reached()` — an **unconditional** `AtomicU64::fetch_add`
//!      on a process-shared line,
//!   3. `GC_SAFEPOINT_PENDING.with(Cell::get)` — a thread-local read, and on
//!      Darwin **a call to `_tlv_get_addr`**, Mach-O having no local-exec TLS
//!      model,
//!   4. `schedule::gc_schedule_enabled()` — a second `OnceLock` acquire load,
//!
//! all of it behind an out-of-line `extern "C"` call the user module cannot
//! inline. Measured on the quiet bench host at ~3 ns per back-edge, which is
//! exactly the regression #7721 shipped when it turned the polls on by default:
//! `churn_alloc` 0.36 s → 0.42, `push_cls` 0.34 → 0.40, `push_num` 0.13 → 0.17,
//! with `js_gc_loop_safepoint` at 8.2 % and `_tlv_get_addr` at 8–9 % of a
//! profile that had driven `_tlv_get_addr` to 0 % two releases earlier (#7469).
//!
//! #7721 was right about the collector — the polls are the only precise
//! nursery-collection point a compute-only program ever reaches, and turning
//! them off costs `tree_wide` 12.4 s. What was wrong was the poll's *price*.
//!
//! # What the word means
//!
//! [`PERRY_GC_POLL_ARMED`] counts the reasons `js_gc_loop_safepoint` has to do
//! more than return. **Zero is a proof that the poll is a no-op**, which is what
//! lets both ends skip the work on a single ordinary load: codegen emits the
//! load inline and branches around the call entirely
//! (`perry-codegen/src/stmt/loops.rs::emit_gc_loop_safepoint`), and the runtime
//! entry point re-checks it so a module compiled by any other path still gets
//! the cheap answer.
//!
//! It is deliberately a conservative SUPERSET. It is process-global, not
//! thread-local — that is the entire point, a thread-local would reintroduce
//! `_tlv_get_addr` — so it counts *threads with a deferral outstanding*, and a
//! poll on thread B can be woken by a deferral on thread A and find nothing to
//! do. That direction is free: the slow path re-reads the real per-thread
//! `GC_SAFEPOINT_PENDING` and returns.
//!
//! The other direction is the only unsound one: the word reading zero while
//! some thread has a deferral outstanding would strand that collection until
//! the next event-loop boundary. That is why no code outside
//! [`super::policy::set_safepoint_pending`] may write `GC_SAFEPOINT_PENDING` —
//! the `Cell` and this counter are one piece of state with two representations,
//! and `pending_transitions_arm_and_disarm` pins them together.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Process-global count of reasons `js_gc_loop_safepoint` must do more than
/// return; see the module docs for the invariant.
///
/// **This symbol is ABI.** `perry-codegen` declares a mirror
/// (`@PERRY_GC_POLL_ARMED`, `external global i32`) in
/// `runtime_decls::arrays` and loads it at every back-edge poll it emits, so
/// the name, the type and the width are part of the compiler/runtime contract
/// rather than an implementation detail. `codegen_mirror_declares_this_symbol`
/// pins the pair.
///
/// **It starts at 1** — the seed. Resolving "should this word be permanently
/// armed?" means asking whether a #7154 stress mode is on, and asking costs
/// exactly what the word exists to avoid, so the process starts armed and the
/// FIRST poll to get through resolves the seed once (see
/// [`resolve_poll_seed`]). A resolved `PERRY_GC_SCHEDULE_SEED` keeps it; every
/// other configuration releases it and the poll goes quiet.
#[no_mangle]
pub static PERRY_GC_POLL_ARMED: AtomicU32 = AtomicU32::new(1);

/// The whole fast path: one relaxed load of an ordinary global. No TLS, no
/// acquire fence, no read-modify-write.
#[inline]
pub(crate) fn poll_armed() -> bool {
    PERRY_GC_POLL_ARMED.load(Ordering::Relaxed) != 0
}

/// Add one reason for the poll to run. Paired with [`disarm_poll`].
#[inline]
pub(crate) fn arm_poll() {
    PERRY_GC_POLL_ARMED.fetch_add(1, Ordering::Relaxed);
}

/// Release one reason.
///
/// Saturating at zero rather than `fetch_sub`: an underflow would wrap to
/// `u32::MAX` and pin the poll permanently armed, which is a silent, permanent
/// return of the exact regression this module removes. Over-arming costs a
/// wasted call; under-arming would be unsound; wrapping would be neither
/// detected nor recoverable, so it is the one outcome made impossible.
#[inline]
pub(crate) fn disarm_poll() {
    let _ = PERRY_GC_POLL_ARMED.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |armed| {
        (armed > 0).then(|| armed - 1)
    });
}

/// Release the startup seed, once, on the first poll that gets through.
///
/// Under `PERRY_GC_SCHEDULE_SEED` the seed is KEPT, and keeping it is
/// load-bearing twice over. The schedule's contract is that it selects among
/// safepoints the deferral flag would skip, not only ones an alloc-point
/// trigger already deferred — with the word released, a seeded run would poll,
/// read zero, skip the call and force nothing, and `schedule_liveness_report`
/// would correctly declare the whole run vacuous. And `note_loop_poll_reached`
/// lives past this gate, so the `loop_polls` figure in `schedule_verdict` is
/// exhaustive exactly when it is read: under a resolved seed.
///
/// Outside the seeded mode the counter is no longer a count of back-edges
/// executed, and [`super::loop_polls_reached`] says so.
pub(crate) fn resolve_poll_seed() {
    // A plain once-flag rather than `std::sync::Once`, for two reasons. It is
    // resettable, so `the_startup_seed_is_kept_only_for_a_resolved_seed` can
    // exercise BOTH directions instead of reading whatever the first poll in
    // the test binary happened to leave behind. And the body only ever
    // releases the seed, so a second thread that races past while the winner
    // is still inside costs one poll taking the slow path and returning —
    // there is nothing here for it to observe half-done.
    if SEED_RESOLVED.swap(true, Ordering::Relaxed) {
        return;
    }
    if !super::schedule::gc_schedule_enabled() {
        disarm_poll();
    }
}

static SEED_RESOLVED: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
pub(crate) fn reset_poll_seed_for_test() {
    SEED_RESOLVED.store(false, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The counter is process-global, so these tests move it and must put it
    /// back; they run under the same `cargo test` process as everything else.
    struct Restore(u32);
    impl Restore {
        fn capture() -> Self {
            Self(PERRY_GC_POLL_ARMED.load(Ordering::Relaxed))
        }
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            PERRY_GC_POLL_ARMED.store(self.0, Ordering::Relaxed);
        }
    }

    #[test]
    fn arm_and_disarm_are_a_counter_not_a_flag() {
        let _r = Restore::capture();
        PERRY_GC_POLL_ARMED.store(0, Ordering::Relaxed);
        assert!(!poll_armed());
        arm_poll();
        arm_poll();
        assert!(poll_armed(), "two arms");
        disarm_poll();
        assert!(
            poll_armed(),
            "one outstanding arm must keep the poll live — a flag would have \
             stranded the second thread's deferred collection here"
        );
        disarm_poll();
        assert!(!poll_armed());
    }

    /// An unbalanced release must not wrap. `u32::MAX` reads as armed forever,
    /// which is the regression this module exists to remove, reintroduced
    /// permanently and silently.
    #[test]
    fn disarm_saturates_at_zero() {
        let _r = Restore::capture();
        PERRY_GC_POLL_ARMED.store(0, Ordering::Relaxed);
        disarm_poll();
        disarm_poll();
        assert_eq!(PERRY_GC_POLL_ARMED.load(Ordering::Relaxed), 0);
        assert!(!poll_armed());
    }

    /// The default is ARMED, and it has to be: the seed is what guarantees the
    /// first poll reaches [`resolve_poll_seed`] at all. A word that started at
    /// zero would never let a seeded run take its own opt-in.
    #[test]
    fn the_process_starts_armed_so_the_first_poll_gets_through() {
        // Not `poll_armed()` — by the time this test runs another test may have
        // resolved the seed. The claim is about the static's initialiser, which
        // is the only thing that can be asserted without ordering.
        static FRESH: AtomicU32 = AtomicU32::new(1);
        assert_eq!(
            FRESH.load(Ordering::Relaxed),
            1,
            "PERRY_GC_POLL_ARMED's initialiser must stay 1; see resolve_poll_seed"
        );
    }
}
