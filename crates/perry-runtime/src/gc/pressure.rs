//! OS memory-pressure entry point (#6184, 2026-07-09 GC audit).
//!
//! Platform hosts (UIKit memory warnings, macOS `DISPATCH_SOURCE_TYPE_
//! MEMORYPRESSURE`, Android `onTrimMemory`, Linux PSI watchers) call
//! `js_gc_memory_pressure` when the OS signals that this process should
//! shed memory NOW — typically the last warning before a jetsam/OOM kill.
//! Before this entry existed, Perry ignored every such signal on every
//! platform and died holding heaps of collectable garbage.
//!
//! Level semantics (loosely mirroring Apple's warn/critical):
//!   1  = warning : collect if safe; and pull the next arena trigger down
//!                  so the very next allocation batch collects even when a
//!                  synchronous collection is not safe right now.
//!   2+ = critical: same, but the collection is a FULL cycle so old-gen
//!                  garbage is reclaimed and idle blocks are handed back
//!                  to the OS (C4b-δ dealloc runs during sweep cleanup).
//!
//! The handler typically fires at a run-loop boundary (JS stack unwound),
//! but the entry defends itself with the same guards the safepoint uses;
//! when collecting here is unsafe, the lowered trigger still guarantees a
//! prompt collection at the next allocation-side check.
//!
//! ★ #7148 disposition: **defer.** This site used to force the conservative
//! native-stack scan unconditionally, on the grounds that "a host may deliver
//! the callback with unspilled locals on the native frames above us". True —
//! but only when there *are* generated frames above us, and the paragraph
//! above says the common case is the opposite: the handler fires at a run-loop
//! boundary with the JS stack unwound. The rationale is now *tested* rather
//! than assumed:
//!
//! * **No active shadow frame** — no generated frame is live on this thread,
//!   so the precise root set is complete (shadow stack empty; globals, module
//!   vars and side tables scanned; runtime helpers hold their temporaries in
//!   `RuntimeHandleScope`). Collect right here with `SkipDisabled` roots:
//!   precise, and the copying minor stays eligible. This is the case hosts
//!   actually deliver, and it is also the case where deferring would be
//!   *worst* — memory pressure arrives when a process is idle, and an idle
//!   process reaches no loop back-edge and pumps no microtasks, so a deferral
//!   would shed nothing at all before the jetsam.
//! * **A generated frame is live above us** — do not scan. Arm the deferral
//!   and return 1, the code this API already documents as "trigger lowered,
//!   collection deferred". A live generated frame is precisely the condition
//!   under which a safepoint *is* reachable: the loop back-edge poll or
//!   microtask-pump boundary that frame will reach drains it precisely.
//!
//! **What if pressure spikes before that safepoint is reached?** The lowered
//! arena trigger is left in place either way, so the alloc-point arm in
//! `gc_check_trigger` stays armed and collects at the next allocation check —
//! identical to the pre-existing `blocked` path, which has always returned 1
//! and relied on exactly that. The deferral adds a faster precise drain in
//! front of that fallback; it removes no backstop.

use super::*;

/// Return codes: 0 = ignored (level 0), 1 = trigger lowered but the
/// collection was deferred (unsafe point, or a generated frame is live above
/// us and a precise safepoint is reachable — #7148), 2 = collected
/// synchronously.
#[no_mangle]
pub extern "C" fn js_gc_memory_pressure(level: u32) -> u32 {
    if level == 0 {
        return 0;
    }
    // Pull the arena trigger down to "collect at the next check" and arm
    // it so the un-armed budget ceiling substitution doesn't override the
    // clamp (see `effective_next_arena_trigger`).
    let total = crate::arena::arena_total_bytes();
    GC_NEXT_TRIGGER_BYTES.with(|c| {
        let clamp = total.saturating_add(1024 * 1024);
        if c.get() > clamp {
            c.set(clamp);
            GC_TRIGGER_ARMED.with(|a| a.set(true));
        }
    });

    let blocked = GC_FLAGS.with(|f| f.get()) & (GC_FLAG_IN_ALLOC | GC_FLAG_SUPPRESSED) != 0
        || gc_blocked_by_unsafe_zone()
        || GC_ROOT_LOCK_DEPTH.with(|depth| depth.get() != 0)
        || gc_budgeted_cycle_active();
    if blocked {
        return 1;
    }

    // #7148: a generated frame is live above us, so this is not a precise
    // point — but it is a point from which a precise one is reachable. Arm the
    // deferral instead of scanning conservatively. `GC_SAFEPOINT_PENDING` is
    // the flag `js_gc_loop_safepoint` polls; for a *critical* level the owed
    // collection must be a full cycle, and `GC_OLD_RECLAIM_PENDING` is exactly
    // the sticky "a full old-gen reclaim is owed" flag `gc_budgeted_due_trigger`
    // reads, so the safepoint drain runs a full mark-sweep rather than a minor.
    if roots::shadow_stack_has_active_frame() {
        if level >= 2 {
            GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(true));
        }
        if !GC_SAFEPOINT_PENDING.with(std::cell::Cell::get) {
            GC_SAFEPOINT_DEFER_ARENA_BASE.with(|base| base.set(total));
            GC_SAFEPOINT_PENDING.with(|p| p.set(true));
        }
        return 1;
    }

    // No generated frame is live on this thread: the precise root set is
    // complete, so NO `force_full_scan`. `conservative_stack_scan_decision()`
    // stays `SkipDisabled` and the copying minor remains eligible.
    if level >= 2 {
        let _ = gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(
            GcTriggerKind::Emergency,
        ));
    } else {
        let _ = gc_collect_minor_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    }
    record_safepoint_drain(SafepointDrainKind::HostPressure);
    2
}
