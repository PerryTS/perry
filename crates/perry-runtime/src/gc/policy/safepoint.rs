//! Precise safepoints: the safepoint-only contract, the moving-minor slack a
//! deferral may grow by, and the moving minor a precise poll
//! (`js_gc_loop_safepoint`) runs.

use super::*;

/// `PERRY_GC_SAFEPOINT_ONLY` — research contract for the native-root modes
/// (`exp/stackmap-viability`): a collection that skips the conservative stack
/// scan consumes only precise roots, and with native stack maps active those
/// roots exist only at mapped PCs — so such a collection may begin only at a
/// declared safepoint; anywhere else it must scan conservatively. Codegen
/// reads the same env to stop emitting statepoints around audited
/// allocate-but-never-reenter helpers; the enforcement in `cycle.rs` is what
/// turns the property from emergent (every possibly-collecting call happens
/// to be mapped) into enforced.
///
/// `1`/`on`/`true` — HEAL: an undeclared precise-root cycle has the
/// conservative scan forced for that cycle (sound: the scan restores
/// liveness, and a conservatively-scanned cycle is non-moving). This is the
/// measuring mode: alloc-point full collections are legitimate today and
/// simply pay the scan.
/// `strict` — PANIC on any undeclared precise-root cycle. This is the gate
/// mode that proves the enforcement is live.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::gc) enum SafepointOnlyContract {
    Off,
    Heal,
    Strict,
}

pub(in crate::gc) fn gc_safepoint_only_contract() -> SafepointOnlyContract {
    use std::sync::OnceLock;
    static CACHED: OnceLock<SafepointOnlyContract> = OnceLock::new();
    *crate::once_init::get_or_init(&CACHED, || {
        safepoint_only_contract_from_value(std::env::var("PERRY_GC_SAFEPOINT_ONLY").ok().as_deref())
    })
}

/// Pure value→contract mapping (#7991), so both directions are testable without
/// touching the process environment. The boolean arm shares the one GC
/// boolean-ish vocabulary; `strict` is this knob's own third state.
pub(in crate::gc) fn safepoint_only_contract_from_value(
    raw: Option<&str>,
) -> SafepointOnlyContract {
    if matches!(
        raw.map(|v| v.trim().to_ascii_lowercase()).as_deref(),
        Some("strict")
    ) {
        return SafepointOnlyContract::Strict;
    }
    if super::env_flag_from_value(raw) {
        return SafepointOnlyContract::Heal;
    }
    SafepointOnlyContract::Off
}

/// Contract enforcement chokepoint, called once at every synchronous
/// collection entry. When an undeclared precise-root collection is about to
/// begin, heal mode returns a scan-override guard that must be held for the
/// WHOLE collection: it flips the thread-local override that every consumer
/// of `conservative_stack_scan_decision()` reads — the root-scan subphase,
/// copying-minor eligibility, and the evacuation verifier alike. A previous
/// revision healed by overriding a local variable inside the root-scan
/// subphase only; copying-minor eligibility still read the global decision,
/// concluded there were no conservative roots to pin, and forced evacuation
/// moved objects that raw native-stack words still pointed at.
pub(in crate::gc) fn contract_scan_heal_guard() -> Option<super::roots::ManualGcScanGuard> {
    if gc_safepoint_only_contract() == SafepointOnlyContract::Off {
        return None;
    }
    if !super::roots::native_stack_maps_active() || GC_AT_DECLARED_SAFEPOINT.with(Cell::get) {
        return None;
    }
    if matches!(
        super::roots::conservative_stack_scan_decision(),
        super::roots::ConservativeStackScanDecision::Scan
    ) {
        return None;
    }
    if gc_safepoint_only_contract() == SafepointOnlyContract::Strict {
        panic!(
            "PERRY_GC_SAFEPOINT_ONLY: precise-root collection began outside \
             a declared safepoint"
        );
    }
    Some(super::roots::ManualGcScanGuard::force_full_scan(
        super::ConservativeScanSite::SafepointContractHeal,
    ))
}

/// RAII marker for a declared-safepoint drain. Nesting-safe: restores the
/// previous value so a poll firing inside a manual `gc()` cannot clear it.
pub(in crate::gc) struct DeclaredSafepointGuard {
    prev: bool,
}

impl DeclaredSafepointGuard {
    pub(in crate::gc) fn enter() -> Self {
        let prev = GC_AT_DECLARED_SAFEPOINT.with(|flag| flag.replace(true));
        Self { prev }
    }
}

impl Drop for DeclaredSafepointGuard {
    fn drop(&mut self) {
        let prev = self.prev;
        GC_AT_DECLARED_SAFEPOINT.with(|flag| flag.set(prev));
    }
}

/// Committed arena bytes a deferred nursery trigger may allocate **past the
/// point at which it was deferred** before the alloc-point non-moving minor
/// runs as the safety valve (Phase 2/3). Loop back-edge polls drain the pending
/// flag every iteration, so the arena never grows near this in normal code; the
/// slack bounds RSS for code that reaches no safepoint before the next trigger —
/// a synchronous loop on a specialized lowering path that doesn't yet emit the
/// poll, or a single mega-expression.
///
/// ★ #7024: this is a SLACK (a delta from the deferral point), not an absolute
/// arena size, and that is the whole point. It was an absolute cap derived by
/// `budget_scaled(_, 1, 4, 2 MB)` — **the same formula as
/// `gc_trigger_absolute_ceiling_bytes()`**. `gc_budgeted_due_trigger()` reports
/// `ArenaBytes` due exactly when `arena_total_bytes() >= trigger`, and the
/// deferral required `arena_total_bytes() < cap`; under any explicit
/// `PERRY_GC_HEAP_LIMIT` the two collapsed to the same number, so the two
/// predicates became exact complements and the deferral was *unreachable* — the
/// copying minor could never run under the very pressure setting the stress
/// matrix used to provoke it (`default` arm: 0 copying minors on all 22 corpus
/// rows). A delta cannot collapse into the trigger, at any heap budget: the
/// first deferral of a cycle is always taken and the arena is allowed a bounded
/// amount of growth to reach a poll.
pub(in crate::gc) const GC_MOVING_DEFER_SLACK_BYTES: usize = 64 * 1024 * 1024;

/// Whether an alloc-point nursery trigger may (still) be deferred to the next
/// precise-root safepoint.
///
/// `deferred_at` is `Some(arena_total_at_the_first_deferral)` while a deferral
/// is outstanding, `None` when none is. The first deferral of a cycle is
/// unconditional — deferring is the *sound* path (it collects with precise,
/// rewritable roots at a real safepoint) and the alloc-point fallback exists
/// only to bound growth when nothing drains the deferral. See
/// `GC_MOVING_DEFER_SLACK_BYTES` for why this is a delta and not an absolute
/// cap (#7024).
#[inline]
pub(in crate::gc) fn moving_defer_within_slack(
    arena_total: usize,
    deferred_at: Option<usize>,
    slack: usize,
) -> bool {
    match deferred_at {
        None => true,
        Some(base) => arena_total < base.saturating_add(slack),
    }
}

/// Phase 1 of the moving-GC project: run a copying (moving) minor at a
/// precise-root safepoint — the outermost microtask-pump boundary, where the
/// JS stack has fully unwound so no live heap pointer sits in an unspilled
/// register. Unlike the alloc-point nursery-churn arm, NO `force_full_scan` is
/// taken: `conservative_stack_scan_decision()` stays `SkipDisabled`, so the
/// copying minor is eligible with precise, rewritable roots and actually MOVES
/// (compacting, O(survivors), no sweep) instead of falling back to the
/// non-moving minor. Trigger detection + re-baseline mirror the nursery-churn
/// arm; this is purely additive (the alloc-point fallback is untouched) and
/// gated by `gc_moving_safepoint_enabled` (**default ON**; the kill switch is
/// `PERRY_GC_MOVING_SAFEPOINT=0`).
///
/// Returns whether the safepoint was HANDLED — false when an entry guard
/// blocked it (mid-allocation, suppressed, unsafe FFI zone, non-zero root-lock
/// depth, active budgeted cycle), true otherwise, including when it was handled
/// and nothing was due. A blocked safepoint consumes no schedule slot, so the
/// caller must not charge it a pacing stride either.
pub(crate) fn gc_safepoint_moving_minor() -> bool {
    gc_safepoint_moving_minor_for(gc_budgeted_due_trigger)
}

/// [`gc_safepoint_moving_minor`] right after a boundary poll that found
/// nothing due ([`gc_boundary_safepoint_poll`]): no mutator code and no
/// collector step ran in between, so the trigger ladder's answer is still
/// "nothing due" and is not derived a second time. The entry guards, the
/// schedule tick, the pending-flag clear and a seed-selected collection run
/// exactly as on the evaluating path.
pub(crate) fn gc_safepoint_moving_minor_nothing_due() -> bool {
    gc_safepoint_moving_minor_for(|| None)
}

#[inline(always)]
fn gc_safepoint_moving_minor_for(due: impl FnOnce() -> Option<BudgetedGcTrigger>) -> bool {
    // Same start guards the budgeted collector uses, minus the (here
    // irrelevant) scanner block: never collect mid-allocation, inside a
    // runtime handle scope, in an unsafe FFI zone, or during a budgeted cycle.
    let flags = GC_FLAGS.with(|f| f.get());
    let in_alloc = flags & (GC_FLAG_IN_ALLOC | GC_FLAG_SUPPRESSED) != 0;
    let unsafe_zone = gc_blocked_by_unsafe_zone();
    let root_lock = GC_ROOT_LOCK_DEPTH.with(|depth| depth.get() != 0);
    let budgeted = gc_budgeted_cycle_active();
    if in_alloc || unsafe_zone || root_lock || budgeted {
        // Blocked right now — leave GC_SAFEPOINT_PENDING set so the next poll
        // retries; do not clear it here.
        //
        // #7909: `budgeted` is the arm that can be PERMANENT. A budgeted cycle
        // started for nursery pressure that this pump's cadence cannot finish
        // rejects every later safepoint here, forever, so it is counted apart
        // from the transient arms.
        if budgeted {
            super::instruments::note_moving_safepoint_blocked_by_budgeted();
        }
        super::instruments::note_moving_safepoint_blocked(in_alloc, unsafe_zone, root_lock);
        return false;
    }
    // We are handling this safepoint (collect or find nothing due): clear the
    // deferral flag set by the alloc-point arm (Phase 2/3).
    //
    // #7154 tooling: this is also the one place the seeded GC-schedule counter
    // advances — after the entry guards, so a safepoint that could not have
    // collected never consumes a schedule slot, and once per handled safepoint
    // whichever arm reached us (loop back-edge poll or microtask-pump boundary).
    // Inert (one cached-`Option` load) unless `PERRY_GC_SCHEDULE_SEED` is set.
    let scheduled = super::schedule::schedule_tick();
    let due = due();
    if !scheduled
        && matches!(
            due,
            Some(BudgetedGcTrigger::ArenaBytes | BudgetedGcTrigger::YoungScavengeCap)
        )
        && super::json_defer::should_defer(true)
    {
        // Keep the poll armed: an allocation-free loop must still expire the
        // allowance. Seed-selected collections never take this branch.
        return true;
    }
    // `set_safepoint_pending`, not a raw `.set(false)`: since #7735 the pending
    // flag is mirrored into the poll arming word, and clearing it behind the
    // mirror would leave the back-edge poll armed forever.
    set_safepoint_pending(false);
    let _declared = DeclaredSafepointGuard::enter();
    // The trigger ladder coalesces the two young resources into the existing
    // precise handoff. Only small occupancy/capacity selects small copying,
    // unless a young large container is alive: its edges need the trace.
    if due == Some(BudgetedGcTrigger::YoungScavengeCap)
        && !scheduled
        && gen_gc_enabled()
        && leaf_service_precedes_copying_cap(
            scavenge_nursery_cap_dueness_bytes(),
            crate::arena::copying_from_space_in_use_bytes(),
        )
        && !crate::arena::young_large_containers_exist()
    {
        super::leaf_service::collect();
        return true;
    }
    let kind = match due {
        // #7909: the nursery cap and the whole-arena trigger are the same
        // collection here — this IS the evacuating collector the cap is for.
        Some(BudgetedGcTrigger::ArenaBytes | BudgetedGcTrigger::YoungScavengeCap) => {
            GcTriggerKind::ArenaBytes
        }
        Some(BudgetedGcTrigger::MallocCount) => GcTriggerKind::MallocCount,
        // ★ #7148: old-gen reclaim used to be the alloc-point arm's business
        // exclusively — it ran a direct full mark-sweep behind a forced
        // conservative scan, because at an allocation point that scan is what
        // makes it sound. Here it is not needed: this is the same precise-root
        // safepoint the nursery minor uses, so run the identical full
        // mark-sweep with `SkipDisabled` roots. The alloc-point arm keeps only
        // the bounded slack valve.
        Some(BudgetedGcTrigger::OldReclaim) => {
            if GC_OLD_RECLAIM_IN_PROGRESS.with(Cell::get)
                || super::schedule::budgeted_old_reclaim_forced()
            {
                return true;
            }
            let _reentry = OldReclaimReentryGuard::enter();
            GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
            super::diagnostic_hooks::site_trigger_decision("safepoint", "OldReclaim");
            super::diagnostic_hooks::site_set_full_site("safepoint_old_reclaim");
            // No `force_full_scan`: roots are precise at this safepoint.
            gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(
                GcTriggerKind::OldGenBytes,
            ))
            .emit_after_current();
            super::record_safepoint_drain(super::SafepointDrainKind::OldReclaim);
            return true;
        }
        _ => {
            // No nursery-pressure trigger is due — nothing to collect here,
            // unless the seeded schedule (#7154 tooling) selected this
            // safepoint, in which case the point of the mode is to collect
            // anyway so an unrooted value moves on its first exposure (at the
            // default `PERRY_GC_SCHEDULE_RATE` of 5% that is one handled
            // safepoint in twenty, at rate 1 every one of them).
            // `gc_force_evacuate_enabled()` is true whenever a seed resolved,
            // so this minor MOVES survivors rather than sweeping in place.
            if !scheduled {
                return true;
            }
            super::schedule::note_schedule_forced_collection();
            GcTriggerKind::ArenaBytes
        }
    };
    let pre_in_use = crate::arena::arena_in_use_bytes();
    let pre_malloc_count = malloc_object_count();
    super::diagnostic_hooks::site_trigger_decision(
        "safepoint",
        match kind {
            GcTriggerKind::MallocCount => "MallocCount",
            _ => "ArenaBytes",
        },
    );
    // #10182: when this minor's promotion can bring the promoted cohort to its
    // bound, its promotion walk records the census facts of the blocks it
    // promotes for the cohort full that would follow at this safepoint.
    let record_census = matches!(kind, GcTriggerKind::ArenaBytes)
        && super::promoted_cohort::promotion_may_reach_bound(
            crate::arena::copying_from_space_in_use_bytes(),
        );
    if record_census {
        super::mark::adopt_census::begin_recording();
    }
    // No `force_full_scan`: roots are precise at this safepoint.
    let outcome = super::gc_collect_minor_with_trigger(GcTriggerSnapshot::capture(kind));
    if record_census {
        super::mark::adopt_census::finish_recording();
    }
    match kind {
        GcTriggerKind::MallocCount => {
            gc_finish_malloc_trigger_collection(pre_malloc_count, pre_in_use, outcome);
        }
        _ => {
            gc_finish_arena_trigger_collection(pre_in_use, outcome);
        }
    }
    // The live-subject counter for every deferral gate: a test that asserts
    // "the conservative valve did not fire" is vacuous unless it can also show
    // the precise collection that replaced it actually ran (CLAUDE.md, four
    // ways a gate cannot fail — #4, the gate runs but its subject never did).
    super::record_safepoint_drain(super::SafepointDrainKind::NurseryMinor);
    run_promoted_cohort_full_if_due();
    super::mark::adopt_census::discard();
    true
}

/// #10182: run the promoted-cohort full (`gc::promoted_cohort`) if the nursery
/// minor this precise safepoint just ran brought the cohort to its bound.
///
/// Same collection the OldReclaim safepoint arm runs — a synchronous full with
/// `SkipDisabled` roots — at the same kind of point, and right after a minor,
/// so an in-place promotion has left no young object for the remembered-set
/// rebuild to find. Returns whether a full ran.
pub(in crate::gc) fn run_promoted_cohort_full_if_due() -> bool {
    if !super::promoted_cohort::full_due() || GC_OLD_RECLAIM_IN_PROGRESS.with(Cell::get) {
        return false;
    }
    let _reentry = OldReclaimReentryGuard::enter();
    let cohort = super::promoted_cohort::promoted_since_full();
    let bound = super::promoted_cohort::bound_bytes();
    let before = old_gen_reclaimable_pressure_bytes().saturating_add(external_side_live_bytes());
    super::diagnostic_hooks::site_trigger_decision("safepoint", "PromotedCohort");
    super::diagnostic_hooks::site_set_full_site("safepoint_promoted_cohort");
    let adopted_before = super::mark::adopt_census::adopted_blocks();
    // #10241: the full's sweep measures how much of what the minor at this
    // safepoint promoted is still reachable (`promoted_cohort::PromotedSurvival`).
    super::promoted_cohort::survival::arm_survival_probe(super::mark::adopt_census::ready_blocks());
    super::mark::adopt_census::begin_adopting();
    // No `force_full_scan`: roots are precise at this safepoint.
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::OldGenBytes))
        .emit_after_current();
    super::mark::adopt_census::discard();
    let survival = super::promoted_cohort::survival::take_survival_probe();
    #[cfg(test)]
    let feed = !super::promoted_cohort::survival::sabotage::unfed();
    #[cfg(not(test))]
    let feed = true;
    let survival_permille = survival.as_ref().and_then(|s| {
        if feed && s.minor_view == super::promoted_cohort::survival::MinorView::Exact {
            super::note_full_measured_promotion_survival(s.promoted_bytes, s.live_bytes)
        } else {
            s.permille()
        }
    });
    let adopted = super::mark::adopt_census::adopted_blocks() - adopted_before;
    let after = old_gen_reclaimable_pressure_bytes().saturating_add(external_side_live_bytes());
    let reclaimed = before.saturating_sub(after);
    let productive = super::promoted_cohort::record_full_yield(cohort, reclaimed);
    if super::diagnostic_hooks::enabled() {
        let (blocks, promoted_by_minor, live_of_promoted, minor_view) =
            survival.as_ref().map_or((0, 0, 0, "none"), |s| {
                (
                    s.blocks,
                    s.promoted_bytes,
                    s.live_bytes,
                    s.minor_view.as_str(),
                )
            });
        eprintln!(
            "[gc-promoted-cohort] full cohort={cohort} bound={bound} reclaimed={reclaimed} \
             productive={productive} adopted_census_blocks={adopted} backoff_shift={} \
             promoted_blocks={blocks} promoted_by_minor={promoted_by_minor} \
             live_of_promoted={live_of_promoted} survival_permille={} minor_view={minor_view} \
             predictor={}",
            super::promoted_cohort::backoff_shift(),
            survival_permille.map_or_else(|| "none".to_string(), |p| p.to_string()),
            super::last_young_survival_permille()
                .map_or_else(|| "none".to_string(), |p| p.to_string())
        );
    }
    true
}

/// The ONLY writer of `GC_SAFEPOINT_PENDING`.
///
/// The flag has a process-global shadow, `gc::poll_arm::PERRY_GC_POLL_ARMED`,
/// because the back-edge poll's fast path may not read a thread-local — on
/// Darwin that is a call to `_tlv_get_addr`, and at 20 M back-edges per
/// `churn_alloc.ts` run it cost 3 ns each (see `gc/poll_arm.rs`). Keeping the
/// two in step is this function's whole job: the `Cell` is the truth for THIS
/// thread, the counter is a conservative superset over all of them, and only
/// transitions move it, so the count is threads-with-a-deferral rather than
/// deferral-events.
pub(crate) fn set_safepoint_pending(pending: bool) {
    GC_SAFEPOINT_PENDING.with(|flag| {
        if flag.get() == pending {
            return;
        }
        flag.set(pending);
        if pending {
            super::arm_poll();
        } else {
            super::disarm_poll();
        }
    });
}

/// Phase 2 of the moving-GC project: codegen emits a call to this at loop
/// back-edges. At a back-edge the loop-body expression has completed, so no
/// heap value lives in an unspilled register (every live value is a named local
/// on the shadow stack): a precise-root safepoint. If moving mode is on and an
/// alloc-point nursery trigger deferred a collection (`GC_SAFEPOINT_PENDING`),
/// drain it here so the copying minor MOVES survivors.
///
/// **The `armed` load is the entire function on the overwhelmingly common
/// path**, and that is a deliberate structure rather than a micro-optimisation.
/// Every allocating loop back-edge in the program lands here — 20 million times
/// in `bench/churn_alloc.ts` — so this is a per-iteration cost of the language,
/// paid whether or not any collection is ever due. #7721 turned the polls on by
/// default (correctly: they are the only precise nursery-collection point a
/// compute-only program reaches) with the body below still doing two `OnceLock`
/// acquire loads, an unconditional atomic increment and a thread-local read,
/// and that cost `churn_alloc` 0.36 s → 0.42, `push_cls` 0.34 → 0.40 and
/// `push_num` 0.13 → 0.17. `gc/poll_arm.rs` carries the measurement and the
/// invariant; `PERRY_GC_POLL_ARMED == 0` is a proof there is nothing to do.
///
/// Codegen normally makes even this call disappear — it loads the same word
/// inline and branches around the call — so reaching this body at all means
/// either the word was armed or the module came from a path that emits the
/// bare call. Both must still work, which is why the check is repeated here
/// rather than delegated to the compiler.
#[cfg(not(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu")))]
#[no_mangle]
pub extern "C" fn js_gc_loop_safepoint() {
    if !super::poll_armed() {
        return;
    }
    js_gc_loop_safepoint_armed(0, 0, 0);
}

/// x86-64 Linux: the same unarmed path written out (load the word through the
/// GOT, compare, return: one instruction fewer than rustc's load, test,
/// branch), so the armed path can hand the body its caller's frame
/// (`roots::walk_origin`) in the argument registers and tail-jump to it. A
/// collection the poll runs then walks from the JS frame that polled instead
/// of stepping the collector's own frames. The shim never owns a frame: both
/// paths leave through `ret` or `jmp` with the stack as the caller made it.
#[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"))]
#[unsafe(naked)]
#[no_mangle]
pub extern "C" fn js_gc_loop_safepoint() {
    core::arch::naked_asm!(
        "mov rax, qword ptr [rip + {armed}@GOTPCREL]",
        "cmp dword ptr [rax], 0",
        "jne 2f",
        "ret",
        "2:",
        "mov rdi, qword ptr [rsp]",
        "lea rsi, [rsp + 8]",
        "mov rdx, rbp",
        "jmp {body}",
        armed = sym super::PERRY_GC_POLL_ARMED,
        body = sym js_gc_loop_safepoint_armed,
    );
}

/// The moving minor of an armed poll, walking from the poll's caller when the
/// shim passed one (`pc != 0`). Offered only here, where a collection can
/// start: most armed polls return before it.
#[inline(always)]
fn gc_safepoint_moving_minor_from(pc: usize, sp: usize, fp: usize) -> bool {
    let origin = super::WalkOrigin { pc, sp, fp };
    let _origin = (pc != 0).then(|| super::EntryWalkOrigin::enter(&origin));
    gc_safepoint_moving_minor()
}

/// Out of line so the hot entry point above stays a load, a compare and a
/// return — no frame, no spills. `(pc, sp, fp)` is the poll's caller frame
/// on x86-64 Linux, where the shim tail-jumps here with it in the argument
/// registers, and `(0, 0, 0)` elsewhere.
#[inline(never)]
extern "C" fn js_gc_loop_safepoint_armed(pc: usize, sp: usize, fp: usize) {
    // Releases the startup seed unless a resolved seed wants the poll kept
    // reachable. Must run before the opt-in check below: a build with the polls
    // killed still has to get the word back to zero, or every back-edge keeps
    // paying for the call.
    super::resolve_poll_seed();
    if !gc_moving_loop_polls_enabled() {
        return;
    }
    // #7604: the only reliable answer to "did the compile-time half take
    // effect". Exhaustive exactly under a resolved seed, which is where
    // `schedule_verdict` reads it — see `resolve_poll_seed` and
    // `loop_polls_reached`.
    super::note_loop_poll_reached();
    // The schedule work all sits inside the `!pending` branch on purpose: a
    // default (mode-off) build reaches exactly the same one cached-bool read
    // and return it did before, and the deferral-drain path below is untouched.
    if !GC_SAFEPOINT_PENDING.with(Cell::get) {
        // The seeded schedule (#7154 tooling) considers polls the deferral flag
        // would skip, so it needs this gate bypassed — and needs it here rather
        // than at the decision point: a schedule cannot select a safepoint this
        // gate already returned from. The decision itself, and the ordinal tick
        // it is a function of, happen inside `gc_safepoint_moving_minor`, past
        // the entry guards. A resolved seed cannot conjure a poll codegen never
        // emitted, so the `gc_moving_loop_polls_enabled()` gate above still
        // applies — see `gc/schedule.rs`.
        if !super::schedule::gc_schedule_enabled() {
            return;
        }
        // ★ #7728, ported: "polls", not "EVERY poll". A poll only becomes a
        // candidate the seed can select once `PERRY_GC_SCHEDULE_ALLOC_KB` of
        // new nursery material has accumulated; unpaced, the rate-1 endpoint
        // cost ~511 µs per loop iteration and turned a 19 s program into a
        // 24-minute one. The stride is a bound on candidates, not a heuristic.
        if !super::schedule::schedule_poll_collection_due(
            crate::arena::copying_from_space_in_use_bytes(),
        ) {
            super::schedule::note_schedule_poll_paced();
            return;
        }
        // Rearm ONLY when the safepoint was handled. A blocked safepoint
        // consumes no schedule slot (see `schedule_tick`'s placement past the
        // entry guards), so charging it a full stride would silently drop the
        // realised density below the requested rate — and a loop that polls
        // while a guard is held would lose candidate after candidate with
        // nothing in the exit summary to say so.
        //
        // Rearm from the level measured AFTER the safepoint, so the next
        // candidate costs a full stride of new allocation on top of whatever
        // survived — see `gc/schedule.rs` for why this is a high-water mark and
        // not a delta.
        if gc_safepoint_moving_minor_from(pc, sp, fp) {
            super::schedule::note_schedule_poll_collection(
                crate::arena::copying_from_space_in_use_bytes(),
            );
        }
        return;
    }
    gc_safepoint_moving_minor_from(pc, sp, fp);
}
