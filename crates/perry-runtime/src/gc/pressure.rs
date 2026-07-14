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

use super::*;

/// Return codes: 0 = ignored (level 0), 1 = trigger lowered but the
/// collection was deferred (unsafe point), 2 = collected synchronously.
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
    // Force the conservative scan for the same reason the alloc-point arm
    // does: a host may deliver the callback with unspilled locals on the
    // native frames above us.
    let _scan = roots::ManualGcScanGuard::force_full_scan();
    if level >= 2 {
        let _ = gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(
            GcTriggerKind::Emergency,
        ));
    } else {
        let _ = gc_collect_minor_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    }
    2
}
