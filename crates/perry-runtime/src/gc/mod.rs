//! Mark-sweep garbage collector for Perry
//!
//! Design:
//! - 8-byte GcHeader prepended to every heap allocation (invisible to callers)
//! - Arena objects (arrays/objects): discovered by walking arena blocks linearly (zero per-alloc tracking cost)
//! - Explicit malloc objects (promises/maps/errors, large closures, and compatibility residents): tracked in MALLOC_STATE
//! - Mark phase: precise thread-local roots + optional conservative stack scan + type-specific tracing
//! - Sweep phase: free malloc objects; arena objects added to free list for reuse
//! - Trigger: only checked on new arena block allocation or explicit gc() call

use std::alloc::{alloc, dealloc, realloc, Layout};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::marker::PhantomData;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex, MutexGuard, OnceLock,
};
use std::time::{Duration, Instant};

mod types;
pub use types::*;
mod policy;
pub use policy::*;
mod telemetry;
pub use telemetry::*;
mod malloc;
pub use malloc::*;
mod roots;
pub use roots::*;
mod layout;
pub use layout::*;
mod trace;
pub(crate) use trace::*;
mod barrier;
pub use barrier::*;
mod copying;
use copying::*;
mod oldgen;
use oldgen::*;
mod verify;
pub use verify::*;

pub fn gc_collect_minor() -> u64 {
    if defer_gc_request(DeferredGcRequest::DirectMinor) {
        return 0;
    }
    gc_collect_minor_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct))
        .emit_after_current()
}

fn gc_collect_minor_with_trigger(trigger: GcTriggerSnapshot) -> GcCollectOutcome {
    // Phase C4b-γ-3: re-entrancy guard. Without this, the evacuation
    // pass's `arena_alloc_gc_old` can trigger `gc_check_trigger` (via
    // `arena.alloc`'s slow-path block-fill) DURING the outer collection
    // cycle. The outer cycle's MARK_SEEDS, CONS_PINNED, and valid_ptrs
    // are all in indeterminate states mid-evac; a recursive
    // `gc_collect_minor` clears them, runs its own mark phase from a
    // mostly-empty C-stack snapshot (we're deep inside the runtime,
    // very few user pointers reachable), evacuates whatever it can find,
    // then returns to the outer cycle which proceeds with corrupt
    // pinning + corrupt seed list. Symptom: bench_evac_heavy's `cache`
    // local gets evacuated by the inner cycle (un-pinned because the
    // inner mark_stack_roots can't see it through the deep-runtime
    // stack), and the outer rewrite walk doesn't update the user's
    // shadow stack slot to point at the new copy → cache.length reads
    // garbage from the FORWARDED slot's first 8 bytes thereafter.
    //
    // Fix: set GC_FLAG_IN_ALLOC for the entire duration of
    // gc_collect_minor. `gc_check_trigger` already early-returns when
    // this bit is set. Any recursive `gc_check_trigger` call from
    // arena_alloc_gc_old / arena_alloc_gc / gc_malloc inside the
    // collection sees the bit and bails. The outer cycle's bookkeeping
    // stays intact.
    let prev_in_alloc = GC_FLAGS.with(|f| {
        let prev = f.get();
        f.set(prev | GC_FLAG_IN_ALLOC);
        prev & GC_FLAG_IN_ALLOC
    });
    if copied_minor_promotion_handoff_due(trigger.kind) {
        let outcome = gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(
            GcTriggerKind::SurvivorPromotionBytes,
        ));
        GC_FLAGS.with(|f| {
            let cur = f.get();
            if prev_in_alloc != 0 {
                f.set(cur | GC_FLAG_IN_ALLOC);
            } else {
                f.set(cur & !GC_FLAG_IN_ALLOC);
            }
        });
        return outcome;
    }
    let mut trace = GcCycleTrace::new(GcCollectionKind::Minor, trigger);
    let start = Instant::now();
    crate::arena::old_pages_begin_gc_cycle();
    let previous_pause_us = gc_last_pause_us();
    let current_rss_bytes = crate::process::get_rss_bytes();
    let evacuation_policy_allowed = gen_gc_evacuate_enabled();
    let force_evacuation = gc_force_evacuate_enabled();
    let old_page_selection = if evacuation_policy_allowed && old_to_young_tracking_complete() {
        select_old_page_defrag_pages(force_evacuation)
    } else {
        OldPageDefragSelection::default()
    };
    // MARK_SEEDS persists across GC cycles. Clear before any try_mark
    // call so trace sees only this cycle's freshly-marked headers.
    clear_mark_seeds();
    if let Some(fast_path) = gc_collect_minor_copying_fast_path(&mut trace, start, trigger.kind) {
        let freed_bytes = fast_path.freed_bytes;
        let elapsed_us = start.elapsed().as_micros() as u64;
        GC_STATS.with(|stats| {
            let mut stats = stats.borrow_mut();
            stats.collection_count += 1;
            stats.total_freed_bytes += freed_bytes;
            stats.last_pause_us = elapsed_us;
        });
        GC_FLAGS.with(|f| {
            let cur = f.get();
            if prev_in_alloc != 0 {
                f.set(cur | GC_FLAG_IN_ALLOC);
            } else {
                f.set(cur & !GC_FLAG_IN_ALLOC);
            }
        });
        if let Some(trace) = trace.as_mut() {
            trace.pause_us = elapsed_us;
            trace.capture_layout_scans();
        }
        return GcCollectOutcome {
            freed_bytes,
            malloc_swept: fast_path.malloc_swept,
            trace,
        };
    }
    clear_mark_seeds();
    let phase_start = trace_phase_start(&trace);
    let valid_ptrs = build_valid_pointer_set();
    trace_phase_record(&mut trace, "build_valid_pointer_set", phase_start);
    let mut evacuation_policy = evacuation_policy_initial_decision(
        valid_ptrs.tenured_nursery_bytes(),
        current_rss_bytes,
        previous_pause_us,
        start.elapsed().as_micros() as u64,
        evacuation_policy_allowed,
        force_evacuation,
        old_to_young_tracking_complete(),
        old_page_selection.selected_pages,
    );
    if let Some(trace) = trace.as_mut() {
        trace.evacuation_policy = evacuation_policy;
    }

    // === MARK PHASE (minor) ===
    // Order matters for the C4b pinning policy:
    //
    //   1. Optional conservative C-stack/register scan first. Those
    //      words cannot be rewritten, so when evacuation is enabled
    //      we pin objects discovered by this phase before any
    //      rewriteable root source can add marks. Default `auto`
    //      mode skips this scan while a precise shadow-stack frame is
    //      active; `PERRY_CONSERVATIVE_STACK_SCAN=full` restores the
    //      legacy always-scan fallback.
    //   2. Mutable root slots (shadow stack + registered globals).
    //      These are real slots we can rewrite after forwarding, so
    //      they stay out of CONS_PINNED.
    //   3. Mutable registered scanners. These expose runtime-owned
    //      slots and are revisited by the forwarding rewrite pass, so
    //      they also stay out of CONS_PINNED.
    //   4. Legacy Rust/FFI scanners. Their API exposes copied f64
    //      values only; when evacuation is enabled the scanner
    //      callbacks pin each discovery directly.
    //
    // Pinning only root-direct discoveries keeps heap-field reachability
    // movable: heap fields are handled later by the reference-rewrite
    // pass.
    let phase_start = trace_phase_start(&trace);
    let conservative_scan_decision = conservative_stack_scan_decision();
    let conservative_root_stats =
        mark_stack_roots_for_decision(&valid_ptrs, conservative_scan_decision);
    // CONS_PINNED is only consumed by `evacuate_tenured_nursery_objects`.
    // Stage 1 keeps the low-pressure path from doing the pinning walk.
    let consider_evacuation = evacuation_policy.considered;
    let conservative_pin_stats = if consider_evacuation
        && matches!(
            conservative_scan_decision,
            ConservativeStackScanDecision::Scan
        ) {
        pin_currently_marked_as_conservative()
    } else {
        ConservativePinTraceStats::default()
    };
    mark_mutable_root_slots(
        &valid_ptrs,
        trace.as_mut().map(|trace| &mut trace.shadow_roots),
    );
    mark_mutable_registered_roots(&valid_ptrs);
    let legacy_root_stats = mark_registered_roots(&valid_ptrs, consider_evacuation);
    if let Some(trace) = trace.as_mut() {
        trace.conservative_root_count = conservative_root_stats.root_count;
        trace.conservative_pinned = conservative_pin_stats.pinned_roots;
        trace.conservative_pinned_bytes = conservative_pin_stats.pinned_bytes;
        trace.legacy_copy_only_scanner_pinned = legacy_root_stats;
    }
    trace_phase_record(&mut trace, "root_marking", phase_start);
    let phase_start = trace_phase_start(&trace);
    let remembered_set = mark_remembered_set_roots(&valid_ptrs);
    trace_phase_record(&mut trace, "remembered_set_marking", phase_start);
    if let Some(trace) = trace.as_mut() {
        trace.remembered_set = remembered_set;
    }
    let phase_start = trace_phase_start(&trace);
    trace_marked_objects_minor(&valid_ptrs);
    trace_phase_record(&mut trace, "trace_worklist", phase_start);
    let phase_start = trace_phase_start(&trace);
    let block_persist = mark_block_persisting_arena_objects(&valid_ptrs);
    trace_phase_record(&mut trace, "block_persistence", phase_start);
    if let Some(trace) = trace.as_mut() {
        trace.block_persist = block_persist;
    }
    // Phase C4b-γ-2 makes evacuation correctness-safe: the
    // post-evac `rewrite_forwarded_references` walk visits every
    // reference site we own (shadow stack + module globals + every
    // marked heap object's fields) and rewrites pointers to
    // forwarded objects. The transitive-pinning safety valve
    // formerly here is no longer needed — non-pinned tenured
    // objects are now genuine evacuation candidates and the bench
    // RSS win lands accordingly.

    // === AGE-BUMP PASS (gen-GC Phase C4) ===
    // Folded into the sweep walk via `sweep_with_age_bump(true)` below.
    // Each general-arena object header was walked twice per minor GC: once
    // here for HAS_SURVIVED/TENURED bookkeeping, once in sweep for the
    // mark/free decision. With ~1.6M objects per cycle in
    // perf-comprehensive that doubled the per-cycle header-touch cost; the
    // merged walk halves it. Aging applies to nursery only (gated on
    // `block_idx < general_block_count()` inside the merged walk), matching
    // the original `pointer_in_old_gen` skip.
    //
    // Two-bit aging (HAS_SURVIVED → TENURED) gives PROMOTION_AGE=2:
    //   - First survival:  set HAS_SURVIVED.
    //   - Second survival: set TENURED, clear HAS_SURVIVED.
    //
    // Tenured objects are skipped by `drain_trace_worklist_minor` on
    // subsequent minor GCs — bounded by the time-win generational design
    // promises. They stay PHYSICALLY in nursery (no copying) so RSS
    // doesn't drop until Phase C4b lands real evacuation.

    // === EVACUATION PASS (Phase C4b-β + C4b-γ-2, auto-policy) ===
    // Copy productive sets of non-pinned tenured nursery objects into
    // OLD_ARENA and install short-lived forwarding pointers in the
    // original nursery slots. After owned references are rewritten and
    // optionally verified, those original stubs release FORWARDED so sweep
    // can reclaim them. Stage 2 runs after mark/trace/block-persist so
    // the policy uses measured movable bytes, block-reclaimable candidate
    // bytes, retained forwarded stubs, pinned bytes, RSS, and pause
    // telemetry instead of a simple env-var opt-in.
    if evacuation_policy.considered {
        let snapshot = evacuation_policy_snapshot_after_mark(
            evacuation_policy.snapshot,
            evacuation_policy.force,
            start.elapsed().as_micros() as u64,
            &old_page_selection,
        );
        evacuation_policy = evacuation_policy_final_decision(evacuation_policy, snapshot);
    } else {
        evacuation_policy.snapshot.pre_evac_pause_us = start.elapsed().as_micros() as u64;
    }
    if let Some(trace) = trace.as_mut() {
        trace.evacuation_policy = evacuation_policy;
    }
    let mut evacuation = EvacuationTraceStats::default();
    let mut evacuation_sticky = StickyRememberedSet::default();
    if evacuation_policy.enabled {
        let phase_start = trace_phase_start(&trace);
        let mut evacuated_new_headers = Vec::new();
        let mut evacuated_original_headers = Vec::new();
        evacuation = evacuate_tenured_nursery_objects_collecting(
            evacuation_policy.force,
            &mut evacuated_new_headers,
            &mut evacuated_original_headers,
        );
        let old_page_evacuation = evacuate_selected_old_pages_collecting(
            &old_page_selection.pages,
            &mut evacuated_new_headers,
            &mut evacuated_original_headers,
        );
        evacuation.objects = evacuation
            .objects
            .saturating_add(old_page_evacuation.objects);
        evacuation.bytes = evacuation.bytes.saturating_add(old_page_evacuation.bytes);
        evacuation.moved_objects = evacuation
            .moved_objects
            .saturating_add(old_page_evacuation.moved_objects);
        evacuation.moved_bytes = evacuation
            .moved_bytes
            .saturating_add(old_page_evacuation.moved_bytes);
        evacuation.old_page_moved_objects = old_page_evacuation.old_page_moved_objects;
        evacuation.old_page_moved_bytes = old_page_evacuation.old_page_moved_bytes;
        trace_phase_record(&mut trace, "evacuation", phase_start);
        if evacuation.objects > 0 {
            let phase_start = trace_phase_start(&trace);
            rewrite_forwarded_references(
                &valid_ptrs,
                trace.as_mut().map(|trace| &mut trace.shadow_roots),
            );
            evacuation_sticky =
                rebuild_evacuated_old_to_young_remembered_set(&evacuated_new_headers);
            trace_phase_record(&mut trace, "reference_rewrite", phase_start);
            if gc_verify_evacuation_enabled() {
                let phase_start = trace_phase_start(&trace);
                verify_evacuated_no_stale_forwarded_refs(&valid_ptrs);
                trace_phase_record(&mut trace, "evacuation_verify", phase_start);
            }
            let released = release_evacuated_original_forwarding_stubs(&evacuated_original_headers);
            evacuation.released_original_objects = released.released_original_objects;
            evacuation.released_original_bytes = released.released_original_bytes;
            evacuation.released_original_reusable_bytes = released.released_original_reusable_bytes;
            evacuation.released_original_returned_bytes = released.released_original_returned_bytes;
        }
    }

    // === SWEEP PHASE ===
    // `do_age_bump = true` folds the per-object HAS_SURVIVED / TENURED
    // update into this same walk (see comment block above the removed
    // dedicated age-bump pass).
    let phase_start = trace_phase_start(&trace);
    let sweep = sweep_with_age_bump(true);
    trace_phase_record(&mut trace, "sweep", phase_start);
    let freed_bytes = sweep.freed_bytes;
    evacuation.retained_forwarded_stub_objects = sweep.retained_forwarded_stub_objects;
    evacuation.retained_forwarded_stub_bytes = sweep.retained_forwarded_stub_bytes;
    maybe_print_evacuation_policy_diag(evacuation_policy, evacuation);
    if let Some(trace) = trace.as_mut() {
        trace.evacuation = evacuation;
        trace.sweep = sweep;
        trace.old_pages = crate::arena::old_page_summary();
    }

    // RS clear — see gc_collect_inner for the rationale.
    let phase_start = trace_phase_start(&trace);
    remembered_set_clear();
    evacuation_sticky.restore();
    trace_phase_record(&mut trace, "remembered_set_clear", phase_start);
    // Conservative-pinning is per-cycle; clear so next cycle
    // re-discovers fresh.
    let phase_start = trace_phase_start(&trace);
    CONS_PINNED.with(|s| s.borrow_mut().clear());
    trace_phase_record(&mut trace, "conservative_pin_clear", phase_start);

    #[cfg(target_env = "gnu")]
    {
        let phase_start = trace_phase_start(&trace);
        unsafe {
            libc::malloc_trim(0);
        }
        trace_phase_record(&mut trace, "malloc_trim", phase_start);
    }

    let elapsed_us = start.elapsed().as_micros() as u64;
    GC_STATS.with(|stats| {
        let mut stats = stats.borrow_mut();
        stats.collection_count += 1;
        stats.total_freed_bytes += freed_bytes;
        stats.last_pause_us = elapsed_us;
    });
    // Restore IN_ALLOC to its pre-collection state. Usually this clears
    // the bit (collections fire from contexts where IN_ALLOC was clear);
    // if the outer caller had it set (e.g., we got here via
    // `js_gc()` invoked from a runtime function that already held the
    // flag), preserve their state.
    GC_FLAGS.with(|f| {
        let cur = f.get();
        if prev_in_alloc != 0 {
            f.set(cur | GC_FLAG_IN_ALLOC);
        } else {
            f.set(cur & !GC_FLAG_IN_ALLOC);
        }
    });
    if let Some(trace) = trace.as_mut() {
        trace.pause_us = elapsed_us;
        trace.capture_layout_scans();
    }
    GcCollectOutcome {
        freed_bytes,
        malloc_swept: true,
        trace,
    }
}

#[inline]

pub fn gen_gc_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_GEN_GC").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// Gen-GC Phase C4b: evacuation is policy-driven by default.
/// `PERRY_GEN_GC_EVACUATE=0`, `=false`, or `=off` disables the
/// policy. `=1`, `=true`, and `=on` are accepted for compatibility
/// but mean "allow the auto-policy", not unconditional evacuation.
pub fn gen_gc_evacuate_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_GEN_GC_EVACUATE").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

fn gc_force_evacuate_enabled() -> bool {
    gen_gc_evacuate_enabled()
        && matches!(
            std::env::var("PERRY_GC_FORCE_EVACUATE").as_deref(),
            Ok("1") | Ok("on") | Ok("true")
        )
}

fn gc_verify_evacuation_enabled() -> bool {
    matches!(
        std::env::var("PERRY_GC_VERIFY_EVACUATION").as_deref(),
        Ok("1") | Ok("on") | Ok("true")
    )
}

#[cfg(test)]
fn gc_collect_inner() -> u64 {
    if defer_gc_request(DeferredGcRequest::Collect(GcTriggerKind::Direct)) {
        return 0;
    }
    gc_collect_inner_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct))
        .emit_after_current()
}

fn gc_collect_inner_with_trigger(trigger: GcTriggerSnapshot) -> GcCollectOutcome {
    // Issue #745: clear the per-cycle bytes-bump flag so the next
    // gc-suppressed parse can rebaseline the trigger again. Done at
    // the top so all entry points — full GC, minor GC, manual
    // `gc()`, the malloc-count trigger path — keep the flag in sync.
    GC_TRIGGER_BUMPED.with(|c| c.set(false));
    if gen_gc_enabled() {
        return gc_collect_minor_with_trigger(trigger);
    }
    gc_collect_full_mark_sweep_with_trigger(trigger)
}

fn gc_collect_full_mark_sweep_with_trigger(trigger: GcTriggerSnapshot) -> GcCollectOutcome {
    GC_TRIGGER_BUMPED.with(|c| c.set(false));
    let mut trace = GcCycleTrace::new(GcCollectionKind::Full, trigger);
    let start = Instant::now();
    crate::arena::old_pages_begin_gc_cycle();

    // MARK_SEEDS persists across GC cycles. Clear before any try_mark
    // call so trace sees only this cycle's freshly-marked headers.
    clear_mark_seeds();
    // Build set of valid heap pointers for conservative stack scan validation
    let phase_start = trace_phase_start(&trace);
    let valid_ptrs = build_valid_pointer_set();
    trace_phase_record(&mut trace, "build_valid_pointer_set", phase_start);

    // === MARK PHASE ===

    // 1. Optional conservative stack scan. Default `auto` mode skips
    // this while a precise shadow-stack frame is active; the fallback
    // remains available with `PERRY_CONSERVATIVE_STACK_SCAN=full`.
    let phase_start = trace_phase_start(&trace);
    let conservative_root_stats = mark_stack_roots(&valid_ptrs);

    // 2. Scan mutable roots (shadow stack + registered globals)
    mark_mutable_root_slots(
        &valid_ptrs,
        trace.as_mut().map(|trace| &mut trace.shadow_roots),
    );

    // 3. Run runtime-owned mutable scanners, then legacy copy-only scanners.
    mark_mutable_registered_roots(&valid_ptrs);
    let legacy_root_stats = mark_registered_roots(&valid_ptrs, false);
    if let Some(trace) = trace.as_mut() {
        trace.conservative_root_count = conservative_root_stats.root_count;
        trace.legacy_copy_only_scanner_pinned = legacy_root_stats;
    }
    trace_phase_record(&mut trace, "root_marking", phase_start);

    // 3b. Gen-GC Phase C3: scan remembered set as additional roots.
    //     Old-gen objects that wrote young-gen pointers since the
    //     last collection are recorded here by the write barrier
    //     (gen-gc-plan.md §C). For full GC this is redundant with
    //     the conservative+precise scan that already covered them,
    //     but it's cheap and keeps the dispatch path uniform with
    //     the eventual minor-GC entry. RS is cleared at the end of
    //     collection so the next cycle starts coherent.
    let phase_start = trace_phase_start(&trace);
    let remembered_set = mark_remembered_set_roots(&valid_ptrs);
    trace_phase_record(&mut trace, "remembered_set_marking", phase_start);
    if let Some(trace) = trace.as_mut() {
        trace.remembered_set = remembered_set;
    }

    // 4. Trace from marked roots (iterative worklist)
    let phase_start = trace_phase_start(&trace);
    trace_marked_objects(&valid_ptrs);
    trace_phase_record(&mut trace, "trace_worklist", phase_start);

    // 5. Block-persistence pass: arena blocks survive whole or not at all, so
    //    arena objects sharing a block with a root-reachable object persist
    //    even when not themselves reachable. Their malloc children must stay
    //    alive too (issues #43 / #44).
    let phase_start = trace_phase_start(&trace);
    let block_persist = mark_block_persisting_arena_objects(&valid_ptrs);
    trace_phase_record(&mut trace, "block_persistence", phase_start);
    if let Some(trace) = trace.as_mut() {
        trace.block_persist = block_persist;
    }

    // === SWEEP PHASE ===
    // The sweep walk clears mark bits on surviving objects inline,
    // eliminating 2 redundant heap walks (arena + malloc).
    let phase_start = trace_phase_start(&trace);
    let sweep = sweep_with_age_bump_and_old_reclaim(false, true);
    trace_phase_record(&mut trace, "sweep", phase_start);
    let freed_bytes = sweep.freed_bytes;
    if let Some(trace) = trace.as_mut() {
        trace.sweep = sweep;
        trace.old_pages = crate::arena::old_page_summary();
    }

    // Gen-GC Phase C3: clear the remembered set after sweep. The
    // RS records old→young writes since the previous collection;
    // after a full collection, every young object referenced by
    // an old-gen parent has either been kept alive (via the
    // mark_remembered_set_roots scan above) or is dead and gets
    // swept. Either way the parent's RS entry is no longer
    // load-bearing — the next allocation cycle's barrier emissions
    // will repopulate it as needed.
    let phase_start = trace_phase_start(&trace);
    remembered_set_clear();
    trace_phase_record(&mut trace, "remembered_set_clear", phase_start);

    // Return released glibc heap pages to the kernel. Without this, glibc
    // keeps freed memory in its arena for reuse but never shrinks RSS, so
    // long-running services show unbounded RSS growth from transient
    // allocations (HTTP buffers, JSON parsers, etc.) even though the
    // Perry GC successfully frees the underlying objects.
    // No-op on non-glibc platforms (macOS, musl).
    #[cfg(target_env = "gnu")]
    {
        let phase_start = trace_phase_start(&trace);
        unsafe {
            libc::malloc_trim(0);
        }
        trace_phase_record(&mut trace, "malloc_trim", phase_start);
    }

    let elapsed_us = start.elapsed().as_micros() as u64;

    GC_STATS.with(|stats| {
        let mut stats = stats.borrow_mut();
        stats.collection_count += 1;
        stats.total_freed_bytes += freed_bytes;
        stats.last_pause_us = elapsed_us;
    });
    if let Some(trace) = trace.as_mut() {
        trace.pause_us = elapsed_us;
        trace.capture_layout_scans();
    }
    finish_full_old_reclaim_baseline();
    GcCollectOutcome {
        freed_bytes,
        malloc_swept: true,
        trace,
    }
}

/// A sorted-`Vec`-backed set of valid user-space heap pointers,
/// used to validate candidate addresses found during the conservative
/// stack scan.
///
/// Two-region layout: arena pointers and malloc pointers are stored
/// in *separate* sorted Vecs. The address-sorted arena walker emits
/// `arena_sorted` already in ascending order with no merge required,
/// so finalize only sorts the small `malloc_sorted` tail (typically a
/// few thousand entries) instead of running driftsort's K-way merge
/// across all 1.6 M arena pointers + the malloc tail. The merge phase
/// of the previous single-Vec implementation cost ~80 ms per GC cycle
/// on perf-comprehensive (1.65 M element memcpy through main memory);
/// keeping the regions separate eliminates it entirely.
///
/// `contains` does two binary searches instead of one (~15 ns extra
/// per call), but contains is only called a few times per traced
/// pointer field — bench profile shows < 500k calls per cycle, so
/// the per-call overhead is dwarfed by the merge savings.
///
/// Profiling background: `HashSet<usize>` with 700 k entries was the
/// dominant GC cost in `object_create` — even after pre-sizing the
/// 700 k inserts were ~10-15 ms per collection because of repeated
/// hash computation + cache misses on the bucket array. Sorted-Vec
/// is ~3× faster on this workload at build time and the O(log n)
/// lookup is fast enough that the few thousand stack-scan candidate
/// validations per GC barely move the total.

pub fn gc_init() {
    gc_register_mutable_root_scanner(scan_runtime_handle_roots_mut);
    gc_register_mutable_root_scanner(promise_mutable_root_scanner);
    gc_register_mutable_root_scanner(timer_mutable_root_scanner);
    gc_register_mutable_root_scanner(exception_mutable_root_scanner);
    gc_register_mutable_root_scanner(async_context_mutable_root_scanner);
    gc_register_mutable_root_scanner(async_hooks_mutable_root_scanner);
    gc_register_mutable_root_scanner(shape_cache_mutable_root_scanner);
    gc_register_mutable_root_scanner(crate::regex::scan_last_exec_groups_root_mut);
    gc_register_mutable_root_scanner(crate::array::scan_template_raw_roots_mut);
    gc_register_mutable_root_scanner(crate::perf_hooks::scan_perf_entries_roots_mut);
    gc_register_mutable_root_scanner(transition_cache_mutable_root_scanner);
    gc_register_mutable_root_scanner(overflow_fields_mutable_root_scanner);
    gc_register_mutable_root_scanner(json_parse_mutable_root_scanner);
    gc_register_mutable_root_scanner(intern_table_mutable_root_scanner);
    gc_register_mutable_root_scanner(crate::builtins::scan_console_log_singleton_roots_mut);
    // Issue #841: GC roots for the per-(submodule, export) function
    // singletons + per-submodule namespace stub objects allocated by
    // `node_submodules.rs`. Without this scanner the next GC cycle
    // after first import-binding use would reclaim the singletons
    // (nothing else holds them — they live for the program's lifetime
    // via codegen `getter` calls, not via a user-visible JSValue root).
    gc_register_mutable_root_scanner(
        crate::node_submodules::scan_node_submodule_singleton_roots_mut,
    );
    // Box-capture root scanner (mutable closure captures, esp. the
    // generator state-machine's `__iter` and `__step` boxes that hold
    // the iter object + step closure across awaits).
    gc_register_mutable_root_scanner(crate::r#box::scan_box_roots_mut);
    // Iter-result scratch slot — the async-step fast path stows the
    // generator's most recent yield value here; it stays live until
    // the step driver reads it back.
    gc_register_mutable_root_scanner(crate::promise::scan_iter_result_root_mut);
    // Async-step thunk single-slot cache (build_async_step_thunks).
    gc_register_mutable_root_scanner(crate::promise::scan_async_step_thunk_cache_mut);
    // Closure singleton caches. Captured-closure cache keys mirror closure
    // capture heap words, so copied-minor must rewrite them after moving
    // captured young values or future cache hits miss on stale addresses.
    gc_register_mutable_root_scanner(crate::closure::scan_singleton_closure_roots_mut);
    gc_register_mutable_root_scanner(crate::closure::scan_closure_dynamic_props_roots_mut);
    // Native-module callable export singletons and process stdio stream
    // singletons store heap pointers in TLS caches; keep them live and rewrite
    // them if a copying collection moves their backing allocations.
    gc_register_mutable_root_scanner(crate::object::scan_native_callable_export_roots_mut);
    gc_register_mutable_root_scanner(crate::os::scan_process_stream_singleton_roots_mut);
    // perry/tui hook + state slot pools — they store raw NaN-boxed
    // value bits but the GC has no other way to know which slots hold
    // heap pointers (arrays/objects/strings stashed via setState /
    // useState / useRef). #679 follow-up: pre-fix, an Enter-press in
    // the perry-code demo stored a freshly-concat'd messages array,
    // the next allocation triggered minor GC, and the array was
    // reclaimed because nothing else held it — `messages.map(…)` on
    // the stale pointer produced an empty render.
    gc_register_mutable_root_scanner(crate::tui::hooks::scan_hook_slot_roots_mut);
    gc_register_mutable_root_scanner(crate::tui::state::scan_state_slot_roots_mut);
    #[cfg(feature = "ohos-napi")]
    gc_register_mutable_root_scanner(crate::arkts_callbacks::arkts_callbacks_root_scanner_mut);
}

#[no_mangle]
pub extern "C" fn js_gc_init() {
    gc_init();
}

/// FFI: get GC stats
#[no_mangle]
pub extern "C" fn js_gc_stats(
    out_collections: *mut u64,
    out_freed: *mut u64,
    out_pause_us: *mut u64,
) {
    GC_STATS.with(|stats| {
        let stats = stats.borrow();
        unsafe {
            if !out_collections.is_null() {
                *out_collections = stats.collection_count;
            }
            if !out_freed.is_null() {
                *out_freed = stats.total_freed_bytes;
            }
            if !out_pause_us.is_null() {
                *out_pause_us = stats.last_pause_us;
            }
        }
    });
}

#[cfg(test)]
mod tests;
