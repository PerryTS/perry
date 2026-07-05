use super::*;

/// Hard work budget for ordinary automatic GC steps once the collector is
/// split into resumable phases.
pub const GC_NORMAL_INCREMENTAL_WORK_UNITS: usize = 2_048;
/// Soft telemetry target for ordinary automatic GC steps.
pub const GC_NORMAL_INCREMENTAL_SOFT_PAUSE_US: u64 = 2_000;
/// Hard work budget for allocation-side mutator assist steps.
pub const GC_MUTATOR_ASSIST_WORK_UNITS: usize = 256;
/// Soft telemetry target for allocation-side mutator assist steps.
pub const GC_MUTATOR_ASSIST_SOFT_PAUSE_US: u64 = 500;

/// Runtime-visible classification for GC progress.
///
/// Only `NormalIncremental` and `MutatorAssist` satisfy the low-pause
/// invariant today defined by this contract: bounded by work units, not heap
/// size. Explicit synchronous work and emergency full collections are allowed
/// to be unbounded only because they are separately requested or separately
/// reported.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GcProgressKind {
    NormalIncremental,
    MutatorAssist,
    ExplicitSynchronous,
    ExplicitFull,
    EmergencyFull,
    LegacySynchronous,
}

impl GcProgressKind {
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NormalIncremental => "normal_incremental",
            Self::MutatorAssist => "mutator_assist",
            Self::ExplicitSynchronous => "explicit_synchronous",
            Self::ExplicitFull => "explicit_full",
            Self::EmergencyFull => "emergency_full",
            Self::LegacySynchronous => "legacy_synchronous",
        }
    }

    #[inline]
    pub const fn is_budgeted(self) -> bool {
        matches!(self, Self::NormalIncremental | Self::MutatorAssist)
    }

    #[inline]
    pub const fn report_class(self) -> &'static str {
        match self {
            Self::NormalIncremental | Self::MutatorAssist => "ordinary_budgeted",
            Self::ExplicitSynchronous | Self::ExplicitFull => "explicit",
            Self::EmergencyFull => "emergency",
            Self::LegacySynchronous => "legacy",
        }
    }
}

/// Hard work-unit limit plus a soft pause target for telemetry.
///
/// `None` means the path is intentionally unbounded and must be labeled by its
/// `GcProgressKind`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GcPauseBudget {
    pub work_units: Option<usize>,
    pub pause_us: Option<u64>,
}

impl GcPauseBudget {
    #[inline]
    pub const fn bounded(work_units: usize, pause_us: u64) -> Self {
        Self {
            work_units: Some(work_units),
            pause_us: Some(pause_us),
        }
    }

    #[inline]
    pub const fn unbounded() -> Self {
        Self {
            work_units: None,
            pause_us: None,
        }
    }

    #[inline]
    pub const fn is_bounded(self) -> bool {
        self.work_units.is_some()
    }
}

/// GC progress policy exposed to runtime and trace consumers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GcProgressContract {
    pub normal_step_budget: GcPauseBudget,
    pub assist_budget: GcPauseBudget,
    pub explicit_synchronous_policy: GcPauseBudget,
    pub explicit_full_policy: GcPauseBudget,
    pub emergency_policy: GcPauseBudget,
}

impl GcProgressContract {
    #[inline]
    pub const fn budget_for(self, kind: GcProgressKind) -> GcPauseBudget {
        match kind {
            GcProgressKind::NormalIncremental => self.normal_step_budget,
            GcProgressKind::MutatorAssist => self.assist_budget,
            GcProgressKind::ExplicitSynchronous => self.explicit_synchronous_policy,
            GcProgressKind::ExplicitFull => self.explicit_full_policy,
            GcProgressKind::EmergencyFull => self.emergency_policy,
            GcProgressKind::LegacySynchronous => GcPauseBudget::unbounded(),
        }
    }
}

impl Default for GcProgressContract {
    fn default() -> Self {
        Self {
            normal_step_budget: GcPauseBudget::bounded(
                GC_NORMAL_INCREMENTAL_WORK_UNITS,
                GC_NORMAL_INCREMENTAL_SOFT_PAUSE_US,
            ),
            assist_budget: GcPauseBudget::bounded(
                GC_MUTATOR_ASSIST_WORK_UNITS,
                GC_MUTATOR_ASSIST_SOFT_PAUSE_US,
            ),
            explicit_synchronous_policy: GcPauseBudget::unbounded(),
            explicit_full_policy: GcPauseBudget::unbounded(),
            emergency_policy: GcPauseBudget::unbounded(),
        }
    }
}

/// Return Perry's process-wide GC progress contract.
pub fn gc_progress_contract() -> GcProgressContract {
    GcProgressContract::default()
}

pub(super) const GC_FLAG_IN_ALLOC: u8 = 0b01;
/// Bit 1 of GC_FLAGS — suppression flag (JSON.parse).
pub(super) const GC_FLAG_SUPPRESSED: u8 = 0b10;

thread_local! {
    pub(super) static GC_FLAGS: Cell<u8> = const { Cell::new(0) };
}

/// Threshold: run GC when total arena bytes exceed this.
///
/// Current app-pattern tuning: 128 MB. The earlier 64 MB setting reduced
/// peak RSS on JSON round-trip style workloads, but it also forced a
/// collection in `buffer_transcode` while the benchmark still held a large
/// live set of rows/strings/buffers. That collection could not reclaim enough
/// and pushed the benchmark past the 30s smoke timeout. Returning the initial
/// trigger to 128 MB keeps allocation-heavy transcode and ECS-style bursts out
/// of mid-run GC while JSON parse/stringify remain below the 1.5x Bun gap in
/// the app-pattern matrix. The absolute ceiling below still bounds later
/// adaptive trigger growth at 128 MB after collections have started.
pub(super) const GC_THRESHOLD_INITIAL_BYTES: usize = 128 * 1024 * 1024; // 128 MB
/// Sanity bound on the adaptive step itself. Step growth past 1 GB is
/// only theoretically possible on multi-day services where GC fires
/// rarely; we keep the cap loose here since the *real* peak-RSS
/// guardrail is `GC_TRIGGER_ABSOLUTE_CEILING` below.
pub(super) const GC_THRESHOLD_MAX_BYTES: usize = 1024 * 1024 * 1024; // 1 GB

/// Hard ceiling on the next-GC trigger (arena_total bytes), independent
/// of how productive recent sweeps have been. Without this, the
/// >90%-freed branch doubles the step on every productive collection,
/// > and `next_trigger = new_total + step` lets peak nursery occupancy
/// > grow unboundedly even when most of what we collected was garbage.
/// > On `bench_json_roundtrip` direct (50 iters × ~5 MB / iter, GC fires
/// > 3 times), the step doubled from 64 MB → 67 MB → 134 MB and the
/// > trigger followed it, so peak nursery hit 115 MB at GC #3 — the
/// > dealloc pass from C4b-δ then returned 91 MB to the OS, but the
/// > peak-RSS damage was already done. Capping the trigger at the
/// > initial threshold prevents that runaway: after GC, trigger ≤ 128 MB
/// > regardless of how much step adapted, so peak nursery stays bounded
/// > to roughly initial + one iter's allocation buffer + headroom for
/// > non-arena overhead.
///
/// Floor: even if `arena_total` is already near or past the ceiling
/// (large old-gen + longlived combined live set), keep at least the
/// 16 MB step floor as headroom — `next_trigger = max(new_total + 16 MB,
/// min(new_total + step, ceiling))`. This avoids GC thrash when the
/// non-nursery component of arena_total alone exceeds the ceiling.
///
/// 2026-05-02 raise from 64 MB → 128 MB: ECS perf-comprehensive's
/// allocation-heavy benches (10k two-comp + sync, 5k × 3 cmds) hit
/// the 64 MB cap mid-round, then the >25%-freed branch halved the
/// step to 16 MB, so the next trigger landed ~16 MB above the post-
/// GC working set — well within a single round's allocation budget.
/// Result: 1-2 mid-round GCs per bench, the worst of which spent
/// 60 ms inside `mark_block_persisting_arena_objects` force-marking
/// + tracing 40 k newly-allocated objects in the recent window.
/// Doubling the cap lets productive sweeps accumulate full
/// `step` headroom (up to 128 MB) before the next trigger, which
/// shifts those GC events out of the measured rounds entirely.
/// `bench_json_roundtrip`-class workloads still bounded — they
/// finish under 128 MB peak and fire ≤2 GCs total.
///
/// Workloads unaffected: `07_object_create` / `12_binary_trees` /
/// `bench_gc_pressure` all fit their working sets under 64 MB and
/// fire GC at most once. The cap only changes behavior when the step
/// would otherwise have pushed the trigger past the initial threshold,
/// which is exactly the bench-RSS scenario this is targeting.
pub(super) const GC_TRIGGER_ABSOLUTE_CEILING: usize = 128 * 1024 * 1024;

thread_local! {
    /// Lower bound for the next GC trigger. Bumped after each
    /// `gc_collect_inner` based on collection effectiveness (see the
    /// adaptive logic in `gc_check_trigger`).
    ///
    /// The initial value is `GC_THRESHOLD_INITIAL_BYTES` (128MB —
    /// chosen so that the 96MB working set of a 1M-iter object_create
    /// or binary_trees benchmark fits under the threshold and pays
    /// zero GC cost). After every collection, if the sweep freed >75%
    /// of arena bytes, the per-program "step" is doubled (capped at
    /// 1GB) so subsequent allocation bursts don't pay GC overhead just
    /// because they re-cross the same line. For hot `new ClassName()`
    /// loops where every object dies between GC cycles, this means
    /// the FIRST burst pays for at most one collection and the rest
    /// run GC-free.
    ///
    /// If a sweep frees <25%, the step is halved (down to a 16MB
    /// floor) so live-set-bound programs don't grow their working
    /// set unboundedly between collections.
    pub(super) static GC_NEXT_TRIGGER_BYTES: std::cell::Cell<usize> =
        const { std::cell::Cell::new(GC_THRESHOLD_INITIAL_BYTES) };

    /// Per-program adaptive GC step. Doubles (up to MAX) when sweeps
    /// are mostly-garbage; halves (down to 16MB) when sweeps reclaim
    /// little. Used to compute the next trigger after each GC as
    /// `post_total + step`.
    pub(super) static GC_STEP_BYTES: std::cell::Cell<usize> =
        const { std::cell::Cell::new(GC_THRESHOLD_INITIAL_BYTES) };

    /// Lower bound for the next malloc-count-based GC trigger. After each
    /// collection, this is reset to `survivor_count + GC_MALLOC_COUNT_STEP`
    /// so that programs with large legitimate live sets (>10k tracked
    /// malloc objects) don't GC-thrash on every subsequent allocation.
    /// See `gc_check_trigger` for the update rule.
    pub(super) static GC_NEXT_MALLOC_TRIGGER: std::cell::Cell<usize> =
        const { std::cell::Cell::new(100_000) };

    /// Issue #745: track whether a medium-or-larger parse already
    /// raised `GC_NEXT_TRIGGER_BYTES` this GC cycle. Cleared in
    /// `gc_collect_inner` whenever a real collection runs.
    pub(super) static GC_TRIGGER_BUMPED: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };

    /// Issue #745: snapshot of `arena_total_bytes()` at the most
    /// recent `gc_suppress` call. Used by `gc_bump_malloc_trigger`
    /// to compute the suppressed window's arena growth.
    pub(super) static GC_PRE_SUPPRESS_BYTES: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };

    /// Non-generational full GC cannot compact a block that still contains
    /// the just-returned parse result. When tiny parse churn crosses the
    /// in-use pressure guard, collect at the next parse boundary instead of
    /// immediately after the current parse, so the previous result has had a
    /// chance to fall out of the shadow roots.
    pub(super) static GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

pub(super) const GC_SUPPRESSED_TINY_PARSE_BYTES: usize = 1024 * 1024;
pub(super) const GC_SUPPRESSED_TINY_PARSE_IN_USE_TRIGGER_BYTES: usize = 48 * 1024 * 1024;
pub(super) const GC_SUPPRESSED_TINY_PARSE_FULL_GC_IN_USE_TRIGGER_BYTES: usize = 24 * 1024 * 1024;

pub(super) fn gc_suppressed_parse_is_tiny(parse_growth: usize) -> bool {
    parse_growth <= GC_SUPPRESSED_TINY_PARSE_BYTES
}

pub(super) fn gc_bump_arena_trigger_target(
    bytes_now: usize,
    step: usize,
    is_tiny_parse: bool,
) -> usize {
    let bytes_step = step.min(GC_THRESHOLD_INITIAL_BYTES);
    let target = bytes_now.saturating_add(bytes_step);
    if is_tiny_parse {
        target.min(GC_TRIGGER_ABSOLUTE_CEILING)
    } else {
        target
    }
}

/// Initial step for the malloc-count-based GC trigger. Adaptive: doubles
/// when >75% of malloc objects are garbage (loop-scoped temporaries),
/// halves when <25% are garbage (large live set). Capped at
/// `GC_MALLOC_COUNT_STEP_MAX` to bound memory between collections.
///
/// Originally a single hardcoded threshold (`GC_MALLOC_COUNT_THRESHOLD`);
/// issue #34 showed that triggering GC from `gc_malloc` (needed for
/// malloc-heavy workloads that don't push arena blocks — e.g.
/// @perry/postgres's `parseBigIntDecimal` bigint chain) combined with a
/// hardcoded threshold would thrash for any program whose live set
/// exceeded the threshold. Making it a per-cycle step fixes that.
///
/// Issue #58: the constant 10k step caused ~100 GC cycles for 500k-iter
/// string-concat loops where almost every object is dead. Adaptive
/// doubling ramps the step to 160k+ after a few mostly-garbage sweeps,
/// cutting GC cycles from ~100 to ~10.
pub(super) const GC_MALLOC_COUNT_STEP_INITIAL: usize = 100_000;
pub(super) const GC_MALLOC_COUNT_STEP_MAX: usize = 2_000_000;
pub(super) const GC_MALLOC_COUNT_STEP_MIN: usize = 10_000;

thread_local! {
    /// Per-program adaptive malloc-count step. Mirrors `GC_STEP_BYTES`
    /// behaviour: doubles when mostly-garbage, halves when mostly-live.
    pub(super) static GC_MALLOC_COUNT_STEP: std::cell::Cell<usize> =
        const { std::cell::Cell::new(GC_MALLOC_COUNT_STEP_INITIAL) };
}

/// #6010: external side-buffer GC pressure. Map entry arrays and Set element
/// arrays are raw `std::alloc` allocations reachable only through a tiny
/// arena header — invisible to every trigger input (arena bytes, malloc
/// object count, old-gen bytes). A workload that churns large Maps/Sets
/// without arena pressure therefore never collected, and once a dead
/// header was conservatively pinned across two cycles it tenured into the
/// old generation, whose reclaim pressure counted only its 16 header bytes
/// — the multi-megabyte buffer leaked for the life of the process (issue
/// #6010: 1.4 GB RSS across a benchmark suite whose live heap never left
/// ~20 MB).
///
/// Two counters close the hole:
/// - ALLOC CHURN (`GC_EXTERNAL_SIDE_ALLOC_PENDING`): every
///   `GC_EXTERNAL_SIDE_ALLOC_STEP` bytes of fresh external allocation pokes
///   `gc_check_trigger()`, so collections happen at all in arena-quiet
///   Map/Set workloads.
/// - LIVE BYTES (`GC_EXTERNAL_SIDE_LIVE_BYTES`): maintained by the
///   alloc/realloc sites and the GC finalizers, and added to `old_in_use`
///   wherever old-reclaim pressure is computed — so tenured-then-dead
///   collections escalate to the full mark-sweep (whose old-gen sweep runs
///   the Map/Set side-allocation finalizers) instead of leaking.
const GC_EXTERNAL_SIDE_ALLOC_STEP: usize = 16 * 1024 * 1024;

thread_local! {
    static GC_EXTERNAL_SIDE_ALLOC_PENDING: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static GC_EXTERNAL_SIDE_LIVE_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Live bytes currently held by external Map/Set side buffers on this thread.
#[inline]
pub(super) fn external_side_live_bytes() -> usize {
    GC_EXTERNAL_SIDE_LIVE_BYTES.with(Cell::get)
}

/// Record `bytes` of fresh external side-buffer allocation (Map entries /
/// Set elements — creation or growth delta) and poke the trigger check when
/// the accumulated churn window fills. Callers must invoke this only when
/// the owning collection header is in a consistent state: a triggered cycle
/// scans conservatively at this call point (`gc_check_trigger`'s direct
/// arms use `force_full_scan`), which also keeps it non-moving, so raw
/// header pointers held by the caller stay valid across the call.
pub(crate) fn gc_note_external_side_alloc(bytes: usize) {
    GC_EXTERNAL_SIDE_LIVE_BYTES.with(|c| c.set(c.get().saturating_add(bytes)));
    let due = GC_EXTERNAL_SIDE_ALLOC_PENDING.with(|c| {
        let now = c.get().saturating_add(bytes);
        if now >= GC_EXTERNAL_SIDE_ALLOC_STEP {
            c.set(0);
            true
        } else {
            c.set(now);
            false
        }
    });
    if due {
        gc_check_trigger();
    }
}

/// Record that a Map/Set side buffer of `bytes` was freed (GC finalizer).
pub(crate) fn gc_note_external_side_free(bytes: usize) {
    GC_EXTERNAL_SIDE_LIVE_BYTES.with(|c| c.set(c.get().saturating_sub(bytes)));
}

#[inline]
pub(super) fn gc_trace_enabled() -> bool {
    #[cfg(test)]
    if GC_TRACE_TEST_FORCE.with(Cell::get) {
        return true;
    }

    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        matches!(
            std::env::var("PERRY_GC_TRACE").as_deref(),
            Ok("1") | Ok("on") | Ok("true")
        )
    })
}

#[cfg(test)]
thread_local! {
    static GC_TRACE_TEST_FORCE: Cell<bool> = const { Cell::new(false) };
}

#[cfg(test)]
pub(super) struct TestGcTraceCaptureGuard {
    previous: bool,
}

#[cfg(test)]
impl TestGcTraceCaptureGuard {
    pub(super) fn force_enabled() -> Self {
        let previous = GC_TRACE_TEST_FORCE.with(|force| {
            let previous = force.get();
            force.set(true);
            previous
        });
        clear_test_last_gc_trace_json();
        Self { previous }
    }
}

#[cfg(test)]
impl Drop for TestGcTraceCaptureGuard {
    fn drop(&mut self) {
        GC_TRACE_TEST_FORCE.with(|force| force.set(self.previous));
        clear_test_last_gc_trace_json();
    }
}

#[derive(Clone, Copy)]
pub(super) enum GcCollectionKind {
    Minor,
    Full,
}

impl GcCollectionKind {
    #[cfg(feature = "diagnostics")]
    #[inline]
    pub(super) fn as_str(self) -> &'static str {
        match self {
            GcCollectionKind::Minor => "minor",
            GcCollectionKind::Full => "full",
        }
    }

    #[inline]
    pub(super) const fn ffi_code(self) -> u32 {
        match self {
            GcCollectionKind::Minor => 1,
            GcCollectionKind::Full => 2,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum GcTriggerKind {
    ArenaBytes,
    MallocCount,
    OldGenBytes,
    SurvivorPromotionBytes,
    Emergency,
    Manual,
    Direct,
}

impl GcTriggerKind {
    #[cfg(feature = "diagnostics")]
    #[inline]
    pub(super) fn as_str(self) -> &'static str {
        match self {
            GcTriggerKind::ArenaBytes => "arena_bytes",
            GcTriggerKind::MallocCount => "malloc_count",
            GcTriggerKind::OldGenBytes => "old_gen_bytes",
            GcTriggerKind::SurvivorPromotionBytes => "survivor_promotion_bytes",
            GcTriggerKind::Emergency => "emergency",
            GcTriggerKind::Manual => "manual",
            GcTriggerKind::Direct => "direct",
        }
    }

    #[inline]
    pub(super) const fn ffi_code(self) -> u32 {
        match self {
            GcTriggerKind::ArenaBytes => 1,
            GcTriggerKind::MallocCount => 2,
            GcTriggerKind::OldGenBytes => 3,
            GcTriggerKind::SurvivorPromotionBytes => 4,
            GcTriggerKind::Manual => 5,
            GcTriggerKind::Direct => 6,
            GcTriggerKind::Emergency => 7,
        }
    }

    #[inline]
    pub(super) const fn progress_kind(self, collection_kind: GcCollectionKind) -> GcProgressKind {
        match (self, collection_kind) {
            (GcTriggerKind::Emergency, GcCollectionKind::Full) => GcProgressKind::EmergencyFull,
            (GcTriggerKind::Manual, GcCollectionKind::Full) => GcProgressKind::ExplicitFull,
            (GcTriggerKind::Manual, GcCollectionKind::Minor) => GcProgressKind::ExplicitSynchronous,
            (
                GcTriggerKind::ArenaBytes
                | GcTriggerKind::MallocCount
                | GcTriggerKind::OldGenBytes
                | GcTriggerKind::SurvivorPromotionBytes
                | GcTriggerKind::Emergency
                | GcTriggerKind::Direct,
                _,
            ) => GcProgressKind::LegacySynchronous,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum DeferredGcRequest {
    None,
    CheckTrigger,
    DirectMinor,
    Collect(GcTriggerKind),
}

impl DeferredGcRequest {
    #[inline]
    pub(super) fn merge(self, next: DeferredGcRequest) -> DeferredGcRequest {
        use DeferredGcRequest::*;
        match (self, next) {
            (None, request) => request,
            (request, None) => request,
            (Collect(GcTriggerKind::Manual), _) | (_, Collect(GcTriggerKind::Manual)) => {
                Collect(GcTriggerKind::Manual)
            }
            (Collect(kind), _) => Collect(kind),
            (_, Collect(kind)) => Collect(kind),
            (DirectMinor, _) | (_, DirectMinor) => DirectMinor,
            (CheckTrigger, CheckTrigger) => CheckTrigger,
        }
    }
}

#[derive(Clone, Copy)]
#[cfg_attr(not(feature = "diagnostics"), allow(dead_code))]
pub(super) struct GcStepSnapshot {
    pub(super) arena_step_bytes: usize,
    pub(super) next_arena_trigger_bytes: usize,
    pub(super) malloc_step: usize,
    pub(super) next_malloc_trigger: usize,
    pub(super) trigger_bumped: bool,
}

impl GcStepSnapshot {
    #[inline]
    pub(super) fn current() -> Self {
        Self {
            arena_step_bytes: GC_STEP_BYTES.with(|c| c.get()),
            next_arena_trigger_bytes: GC_NEXT_TRIGGER_BYTES.with(|c| c.get()),
            malloc_step: GC_MALLOC_COUNT_STEP.with(|c| c.get()),
            next_malloc_trigger: GC_NEXT_MALLOC_TRIGGER.with(|c| c.get()),
            trigger_bumped: GC_TRIGGER_BUMPED.with(|c| c.get()),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct GcTriggerSnapshot {
    pub(super) kind: GcTriggerKind,
    pub(super) steps_before: Option<GcStepSnapshot>,
}

impl GcTriggerSnapshot {
    #[inline]
    pub(super) fn capture(kind: GcTriggerKind) -> Self {
        Self {
            kind,
            steps_before: gc_trace_enabled().then(GcStepSnapshot::current),
        }
    }
}

thread_local! {
    pub(super) static GC_DEFERRED_REQUEST: Cell<DeferredGcRequest> =
        const { Cell::new(DeferredGcRequest::None) };
    pub(super) static GC_OLD_RECLAIM_PENDING: Cell<bool> = const { Cell::new(false) };
    pub(super) static GC_LAST_OLD_RECLAIM_IN_USE_BYTES: Cell<usize> = const { Cell::new(0) };
    /// Re-entrancy guard for the #5476 direct old-gen reclaim driven from
    /// `gc_check_trigger`: the full collection must not recursively trigger
    /// another reclaim if a hook it runs allocates.
    pub(super) static GC_OLD_RECLAIM_IN_PROGRESS: Cell<bool> = const { Cell::new(false) };
}

/// RAII guard that marks a #5476 direct old-gen reclaim in progress so a nested
/// `gc_check_trigger` can't re-enter it. See `GC_OLD_RECLAIM_IN_PROGRESS`.
struct OldReclaimReentryGuard;

impl OldReclaimReentryGuard {
    fn enter() -> Self {
        GC_OLD_RECLAIM_IN_PROGRESS.with(|p| p.set(true));
        Self
    }
}

impl Drop for OldReclaimReentryGuard {
    fn drop(&mut self) {
        GC_OLD_RECLAIM_IN_PROGRESS.with(|p| p.set(false));
    }
}

pub(super) const GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES: usize = 48 * 1024 * 1024;
pub(super) const GC_OLD_GEN_RECLAIM_GROWTH_BYTES: usize = 32 * 1024 * 1024;
pub(super) const GC_COPY_PROMOTION_HANDOFF_MIN_BYTES: usize = 24 * 1024 * 1024;

#[inline]
pub(super) fn defer_gc_request(request: DeferredGcRequest) -> bool {
    let locked = GC_ROOT_LOCK_DEPTH.with(|depth| depth.get() != 0);
    if locked {
        GC_DEFERRED_REQUEST.with(|pending| {
            pending.set(pending.get().merge(request));
        });
    }
    locked
}

pub(super) fn take_deferred_gc_request() -> DeferredGcRequest {
    GC_DEFERRED_REQUEST.with(|pending| {
        let request = pending.get();
        pending.set(DeferredGcRequest::None);
        request
    })
}

pub(super) fn flush_deferred_gc_request() {
    if std::thread::panicking() {
        let _ = take_deferred_gc_request();
        return;
    }
    match take_deferred_gc_request() {
        DeferredGcRequest::None => {}
        DeferredGcRequest::CheckTrigger => gc_check_trigger(),
        DeferredGcRequest::DirectMinor => {
            if gc_blocked_by_unsafe_zone() {
                return;
            }
            gc_collect_minor_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct))
                .emit_after_current();
        }
        DeferredGcRequest::Collect(GcTriggerKind::Manual) => {
            if manual_gc_blocked_by_unsafe_zone() {
                return;
            }
            manual_gc_collect_now();
        }
        DeferredGcRequest::Collect(kind) => {
            if gc_blocked_by_unsafe_zone() {
                return;
            }
            gc_collect_inner_with_trigger(GcTriggerSnapshot::capture(kind)).emit_after_current();
        }
    }
}

pub fn gc_suppress() {
    if !gen_gc_enabled()
        && crate::arena::arena_in_use_bytes()
            >= GC_SUPPRESSED_TINY_PARSE_FULL_GC_IN_USE_TRIGGER_BYTES
    {
        crate::arena::arena_start_fresh_general_block();
    }
    // Issue #745: snapshot arena_total at suppress-start so the
    // matching `gc_bump_malloc_trigger` can size the suppressed
    // window's parse growth and gate the bytes-trigger bump on it.
    GC_PRE_SUPPRESS_BYTES.with(|c| c.set(crate::arena::arena_total_bytes()));
    GC_FLAGS.with(|f| f.set(f.get() | GC_FLAG_SUPPRESSED));
}

/// Resume GC triggers after suppression.
pub fn gc_unsuppress() {
    GC_FLAGS.with(|f| f.set(f.get() & !GC_FLAG_SUPPRESSED));
}

/// Rebaseline the malloc-count AND arena-bytes triggers to the current
/// live set so that objects just created during a GC-suppressed window
/// (e.g. JSON.parse) don't immediately trip a collection on the next
/// allocation.
///
/// Pre-fix: only the malloc-count trigger was bumped. JSON.parse on the
/// 108 MB honest_bench fixture lifts arena_total to ~108 MB, the bytes
/// trigger is still at its initial 128 MB threshold, and the iterate+
/// rebuild pass that immediately follows trips bytes-based GC after
/// only ~20 MB of new allocations. The 4 mark/sweep cycles each walk
/// the entire 400 MB live heap (the records tree dominates) and add
/// ~800 ms of overhead to the workload. Bumping the bytes trigger by
/// the per-program step (initially 128 MB, grows up to 1 GB on
/// mostly-garbage sweep evidence) defers the first GC until the
/// post-parse working set itself doubles — for json_pipeline_full
/// that means iterate+rebuild completes inside one GC cycle instead
/// of four.
pub fn gc_bump_malloc_trigger() {
    let current = MALLOC_STATE.with(|s| s.borrow().objects.len());
    use crate::arena::arena_total_bytes;
    let bytes_now = arena_total_bytes();
    let is_tiny_parse = gc_bump_malloc_trigger_with_snapshot(current, bytes_now);
    if is_tiny_parse {
        let use_gen_gc = gen_gc_enabled();
        let in_use_trigger = if use_gen_gc {
            GC_SUPPRESSED_TINY_PARSE_IN_USE_TRIGGER_BYTES
        } else {
            GC_SUPPRESSED_TINY_PARSE_FULL_GC_IN_USE_TRIGGER_BYTES
        };
        if crate::arena::arena_in_use_bytes() < in_use_trigger {
            return;
        }
        if use_gen_gc {
            if gc_blocked_by_unsafe_zone() {
                GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|pending| pending.set(true));
                return;
            }
            GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|pending| pending.set(true));
            GC_NEXT_TRIGGER_BYTES.with(|trigger| {
                if trigger.get() > bytes_now {
                    trigger.set(bytes_now);
                }
            });
            gc_check_trigger();
        } else {
            crate::arena::arena_start_fresh_general_block();
            GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|pending| pending.set(true));
        }
    }
}

/// Run a full collection that was armed by tiny JSON parse churn.
///
/// This is separate from the raise-only post-parse trigger bump. Full
/// mark-sweep needs the collection to happen before the next suppressed parse,
/// not immediately after the previous one, otherwise the parse result is still
/// rooted and every churn block looks partially live.
pub fn gc_collect_pending_suppressed_parse() {
    let pending = GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|pending| {
        let was_pending = pending.get();
        pending.set(false);
        was_pending
    });
    if !pending {
        return;
    }
    if GC_FLAGS.with(|f| f.get()) & (GC_FLAG_IN_ALLOC | GC_FLAG_SUPPRESSED) != 0
        || gc_blocked_by_unsafe_zone()
    {
        GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|pending| pending.set(true));
        return;
    }

    let total = crate::arena::arena_total_bytes();
    GC_NEXT_TRIGGER_BYTES.with(|trigger| {
        if trigger.get() > total {
            trigger.set(total);
        }
    });
    gc_check_trigger();
}

/// Schedule a collection for the next JSON.parse boundary.
///
/// Direct parse + stringify churn creates a full JS object graph, then walks it
/// immediately. If the arena trigger fires during that stringify, copied-minor
/// has to copy the just-parsed tree even though it dies at the end of the loop
/// body. Deferring the collection to the next parse boundary lets the caller's
/// loop-scope roots clear first, so the collector reclaims the previous tree
/// without promoting or repeatedly copying transient JSON data.
pub fn gc_schedule_parse_boundary_collection_if_pressure() {
    if !gen_gc_enabled() {
        return;
    }
    if crate::arena::arena_in_use_bytes() < GC_SUPPRESSED_TINY_PARSE_IN_USE_TRIGGER_BYTES {
        return;
    }
    GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|pending| pending.set(true));
}

#[inline]
pub(super) fn old_reclaim_pressure_due(old_in_use: usize, baseline: usize) -> bool {
    (old_in_use >= GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES
        && baseline < GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES)
        || old_in_use.saturating_sub(baseline) >= GC_OLD_GEN_RECLAIM_GROWTH_BYTES
}

#[inline]
pub(super) fn copied_minor_promotion_handoff_pressure_due(
    promotable_bytes: usize,
    old_in_use: usize,
    baseline: usize,
) -> bool {
    promotable_bytes >= GC_COPY_PROMOTION_HANDOFF_MIN_BYTES
        && old_reclaim_pressure_due(old_in_use.saturating_add(promotable_bytes), baseline)
}

pub(super) fn copied_minor_promotable_active_survivor_bytes() -> usize {
    let active_range = crate::arena::active_survivor_block_index_range();
    let mut promotable = 0usize;
    crate::arena::arena_walk_objects_with_block_index(|header_ptr, block_idx| {
        if !active_range.contains(&block_idx) {
            return;
        }
        let header = header_ptr as *mut GcHeader;
        unsafe {
            let flags = (*header).gc_flags;
            if flags & GC_FLAG_FORWARDED != 0 {
                return;
            }
            let prior_age = copied_survival_age((*header)._reserved, flags);
            let next_age = prior_age.saturating_add(1);
            if flags & GC_FLAG_TENURED != 0 || next_age >= GC_COPY_PROMOTION_SURVIVALS {
                promotable = promotable.saturating_add((*header).size as usize);
            }
        }
    });
    promotable
}

pub(super) fn copied_minor_promotion_handoff_due(trigger_kind: GcTriggerKind) -> bool {
    if !matches!(
        trigger_kind,
        GcTriggerKind::ArenaBytes | GcTriggerKind::MallocCount
    ) {
        return false;
    }
    if crate::arena::copying_active_survivor_in_use_bytes() < GC_COPY_PROMOTION_HANDOFF_MIN_BYTES {
        return false;
    }
    let promotable = copied_minor_promotable_active_survivor_bytes();
    let old_in_use =
        crate::arena::old_gen_in_use_bytes().saturating_add(external_side_live_bytes());
    let baseline = GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.get());
    copied_minor_promotion_handoff_pressure_due(promotable, old_in_use, baseline)
}

pub(super) fn maybe_schedule_old_reclaim_after_copied_minor() {
    // #6010: external Map/Set side buffers count toward old-gen pressure —
    // a tenured-then-dead Map holds its multi-MB buffer until a full
    // reclaim's old-gen sweep finalizes it, so the buffer bytes must be
    // able to escalate that reclaim.
    let old_in_use =
        crate::arena::old_gen_in_use_bytes().saturating_add(external_side_live_bytes());
    let baseline = GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.get());
    if old_reclaim_pressure_due(old_in_use, baseline) {
        GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(true));
    }
}

pub(super) fn finish_full_old_reclaim_baseline() {
    // Baseline includes external side-buffer bytes (#6010) so the growth
    // delta in `old_reclaim_pressure_due` stays unit-consistent.
    let old_in_use =
        crate::arena::old_gen_in_use_bytes().saturating_add(external_side_live_bytes());
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.set(old_in_use));
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
}

pub(super) fn gc_bump_malloc_trigger_with_snapshot(current: usize, bytes_now: usize) -> bool {
    let step = GC_MALLOC_COUNT_STEP.with(|c| c.get());
    GC_NEXT_MALLOC_TRIGGER.with(|c| c.set(current + step));

    let pre_suppress = GC_PRE_SUPPRESS_BYTES.with(|c| c.get());
    let parse_growth = bytes_now.saturating_sub(pre_suppress);

    // Issue #745: gate the bytes-trigger bump on the suppressed
    // window's parse size, with two regimes:
    //
    //   * Tiny parses (<= 1 MB of arena growth) — the
    //     `test_memory_json_churn` shape: 5 k iters × ~13 KB per
    //     parse into a fragmented arena, where a small parse can still
    //     force one fresh 1 MB block while GC is suppressed. Allow
    //     repeated bumps here, but clamp them to the collector's
    //     absolute trigger ceiling so a tiny parse loop cannot keep
    //     ratcheting the next GC beyond the RSS guardrail. If a
    //     suppressed parse crosses the trigger, the next pre-parse or
    //     normal allocation check sees the trigger still due.
    //
    //   * Medium-or-larger parses (> 1 MB) — the
    //     `json_pipeline_full` and `json_polyglot` shapes: once per
    //     GC cycle, bump the trigger to grant the post-parse
    //     workload a `step` of headroom. The flag clears in
    //     `gc_collect_inner` so the next cycle gets its own bump.
    //     This is what was missing in commit 56818086 — each
    //     iteration of `json_polyglot`'s 50-iter loop bumped the
    //     trigger by another `step`, and after productive
    //     step-doubling that grew toward 1 GB the trigger ratcheted
    //     hundreds of MB above the actual live set (~5 MB) and GC
    //     never fired across the entire run. Peak RSS climbed to
    //     254/411 MB on the lazy-tape path.
    //
    // Also cap the effective step at the *initial* value (64 MB) so
    // post-`73a48ced` step-doubling can't make a single bump grant
    // hundreds of MB of headroom. The original optimization measured
    // `step` at INITIAL on the first call (no prior GC), so the cap
    // is a no-op for the `json_pipeline_full` workload.
    let is_tiny_parse = gc_suppressed_parse_is_tiny(parse_growth);
    if !is_tiny_parse && GC_TRIGGER_BUMPED.with(|c| c.get()) {
        return false;
    }

    let bytes_step = GC_STEP_BYTES.with(|c| c.get());
    let bytes_trigger = gc_bump_arena_trigger_target(bytes_now, bytes_step, is_tiny_parse);
    // Only raise — never lower — so this can't accidentally trip a
    // pending collection that the existing trigger had already armed.
    GC_NEXT_TRIGGER_BYTES.with(|c| {
        if bytes_trigger > c.get() {
            c.set(bytes_trigger);
            if !is_tiny_parse {
                GC_TRIGGER_BUMPED.with(|b| b.set(true));
            }
        }
    });
    is_tiny_parse
}

fn gc_rebaseline_malloc_trigger_to_survivors(mstep: usize) {
    let survivors = MALLOC_STATE.with(|s| s.borrow().objects.len());
    GC_NEXT_MALLOC_TRIGGER.with(|c| c.set(survivors + mstep));
}

fn gc_finish_arena_trigger_collection(pre_in_use: usize, outcome: GcCollectOutcome) -> u64 {
    let sweep_freed_bytes = outcome.freed_bytes;
    let malloc_swept = outcome.malloc_swept;
    let post_in_use = crate::arena::arena_in_use_bytes();

    // Adaptive step:
    //   >90% freed → double (almost all dead — `object_create`-style
    //                        hot loops fit their entire working set
    //                        under the threshold; defer.)
    //   10-90% freed → halve (productive collection — real reclaim
    //                         is possible, so collect again sooner
    //                         to keep the working set bounded;
    //                         16MB floor prevents thrash).
    //   <10% freed → double (live set genuinely large, don't thrash).
    //
    // Issue #179: the halve band was formerly 10-25% only. Before
    // the age-restricted block-persist, collections in the 25-90%
    // band were illusory — block-persist re-marked dead neighbors
    // as live, so "freed" over-counted what was actually reclaimable
    // on subsequent cycles. Keeping step flat there was the correct
    // defensive choice. With v0.5.193's block-persist limited to
    // the last 5 general-arena blocks, "freed" now reflects real
    // sweep effectiveness, and widening the halve band lets the
    // trigger fire often enough for middle blocks to actually
    // reset and RSS to stay bounded. `bench_json_roundtrip` moves
    // into this band: first GC frees ~73% → halve → next trigger
    // ~56MB later → second GC frees more → step halves again →
    // RSS stabilizes instead of growing linearly with iters.
    //
    // The >90% and <10% branches retain the existing "don't thrash"
    // protection (Issue #64 follow-up): both extremes mean the
    // live/garbage ratio is such that collecting sooner is wasted
    // work.
    // Adaptive step, driven by the *larger* of sweep-freed-bytes
    // and the block-reset delta (`pre - post`). `freed_bytes` from
    // the sweep surfaces reclaim potential immediately (before the
    // 2-cycle grace completes); `pre - post` reflects actual block
    // resets landing on subsequent cycles. Using the max keeps the
    // step adaptive to both surfaces of productive collection.
    //
    //   >90% freed → double (near-total sweep; `object_create`-style
    //                        hot loops pay one GC then run free).
    //   25-90% freed → halve (productive — reclaim is meaningful,
    //                         collect again sooner to bound RSS).
    //   10-25% freed → keep (marginal — don't thrash vs. churn).
    //   <10% freed → double (live set genuinely large, defer).
    //
    // Issue #179 driver: formerly the halve band was 10-25% only,
    // which never fired on `bench_json_roundtrip` because typical
    // freed-pct there is 50-80%. With the max-of-two metric AND
    // the age-restricted block-persist (v0.5.193), widening the
    // halve band to 25-90% lets the trigger fire often enough for
    // middle blocks to actually reset, without dropping into the
    // 16MB-floor thrash territory that hurts throughput on
    // moderate workloads. `bench_json_roundtrip` lands here on
    // most cycles (60-80% freed) → step halves → GC fires 3-4×
    // across the 50-iter loop → RSS stabilizes around the live-
    // set size plus the 5-block recent-window headroom.
    //
    // The 16MB floor keeps `object_create`-scale hot loops from
    // thrashing: those workloads land in the >90% band on the
    // first GC and immediately double the step, escaping the
    // halve trajectory after a single cycle.
    let block_reclaim = pre_in_use.saturating_sub(post_in_use);
    let freed = std::cmp::max(block_reclaim, sweep_freed_bytes as usize);
    let mut step = GC_STEP_BYTES.with(|c| c.get());
    let old_step = step;
    if pre_in_use > 0 {
        let pct_freed = (freed * 100) / pre_in_use;
        // 2026-05-02: widen the "double" band from `>90% || <10%` to
        // `>=85% || <10%`. ECS perf-comprehensive's two
        // alloc-heavy benches (10k two-comp, 5k × 3 cmds) sweep
        // at 86-89 % freed, which previously landed in the halve
        // band. Step would shrink 64→32→16 MB across the first
        // two benches, then GC fired every ~16 MB of fresh
        // allocations — a 60 ms `mark_block_persisting_arena_objects`
        // outlier landed mid-measured-round on each refire.
        // Promoting 85-90 % to double lets the step grow to the
        // 128 MB ceiling on the first sweep, the trigger jumps
        // out past the bench's full per-iteration allocation
        // budget, and subsequent GCs fire BETWEEN measured rounds
        // (i.e. invisible to the bench's wall-time counter).
        // `bench_json_roundtrip` lands at 50-80 % freed and is
        // unchanged — it still halves and stabilizes at the floor.
        //
        // With INITIAL == ABSOLUTE_CEILING (128 MB), the post-GC
        // `next_trigger` cap below supersedes doubling above the
        // ceiling; the doubling branch is kept for the bisection
        // escape hatch.
        if !(10..=84).contains(&pct_freed) {
            step = (step * 2).min(GC_THRESHOLD_MAX_BYTES);
        } else if pct_freed >= 25 {
            step = (step / 2).max(16 * 1024 * 1024);
        }
        // 10-25% freed → keep step unchanged (marginal churn).
        GC_STEP_BYTES.with(|c| c.set(step));
        if std::env::var_os("PERRY_GC_DIAG").is_some() {
            eprintln!(
                "[gc-step] pre_in_use={} post_in_use={} sweep_freed={} block_reclaim={} pct={}% step={}→{}",
                pre_in_use, post_in_use, sweep_freed_bytes, block_reclaim, pct_freed, old_step, step
            );
        }
    }
    let new_total = crate::arena::arena_total_bytes();
    // C4b-δ-tune: hard cap on next_trigger so the >90%-freed
    // step-doubling can't drive peak nursery past the initial
    // threshold. Floor: at least 16 MB of headroom past
    // `new_total` so a workload whose post-GC live set already
    // approaches the ceiling doesn't thrash on every fresh
    // allocation.
    let stepped = new_total.saturating_add(step);
    let capped = stepped.min(GC_TRIGGER_ABSOLUTE_CEILING);
    let floor = new_total.saturating_add(16 * 1024 * 1024);
    let next_trigger = std::cmp::max(capped, floor);
    GC_NEXT_TRIGGER_BYTES.with(|c| c.set(next_trigger));
    // Rebaseline the malloc-count trigger only if this collection
    // actually swept malloc objects. Copied-minor arena collections
    // may skip the malloc sweep while count pressure is still below
    // its trigger; moving the trigger in that case would postpone
    // reclamation of already-tracked dead malloc churn.
    if malloc_swept {
        let mstep = GC_MALLOC_COUNT_STEP.with(|c| c.get());
        gc_rebaseline_malloc_trigger_to_survivors(mstep);
    }
    outcome.emit_after_current()
}

fn gc_finish_malloc_trigger_collection(pre_count: usize, outcome: GcCollectOutcome) -> u64 {
    debug_assert!(
        outcome.malloc_swept,
        "malloc-count trigger must sweep malloc objects"
    );
    let survivors = MALLOC_STATE.with(|s| s.borrow().objects.len());
    // Adapt the malloc-count step based on collection effectiveness.
    //
    // Issue #58 insight: in tight allocation loops the conservative
    // stack scanner keeps almost everything alive — GC finds <10%
    // garbage and wastes time walking 100k+ objects. In this regime
    // we should BACK OFF (increase the step) so the loop can finish
    // without GC interference. Once control returns to a higher scope
    // the dead objects will fall off the stack and become collectable.
    //
    // Conversely, when GC reclaims >75% it's working well and can
    // afford to stay at the current cadence or even speed up.
    let mut mstep = GC_MALLOC_COUNT_STEP.with(|c| c.get());
    if pre_count > 0 {
        let freed = pre_count.saturating_sub(survivors);
        let pct_freed = (freed * 100) / pre_count;
        if pct_freed < 15 {
            // GC is nearly useless — quadruple the step to back off fast
            mstep = (mstep * 4).min(GC_MALLOC_COUNT_STEP_MAX);
        } else if pct_freed < 50 {
            // GC is partially effective — double the step
            mstep = (mstep * 2).min(GC_MALLOC_COUNT_STEP_MAX);
        } else if pct_freed > 90 {
            // GC is highly effective — halve the step to collect sooner
            mstep = (mstep / 2).max(GC_MALLOC_COUNT_STEP_MIN);
        }
        // 50-90% freed: keep current step (balanced)
        GC_MALLOC_COUNT_STEP.with(|c| c.set(mstep));
    }
    if outcome.malloc_swept {
        GC_NEXT_MALLOC_TRIGGER.with(|c| c.set(survivors + mstep));
    }
    outcome.emit_after_current()
}

/// Check if automatic GC pressure should pay a bounded assist step.
///
/// Arena and malloc thresholds are heap goals. Crossing them starts or resumes
/// a budgeted cycle. Allocation-side assists spend at most
/// `GC_MUTATOR_ASSIST_WORK_UNITS` and only enter phases that already consume
/// that budget; unsliced phases stay active for host-driven budgeted steps.
pub fn gc_check_trigger() {
    if GC_BUDGETED_STEP_ACTIVE.with(Cell::get) {
        return;
    }
    // Issue #62: single TLS access covers both `in_alloc` and `suppressed`.
    let flags = GC_FLAGS.with(|f| f.get());
    if flags & GC_FLAG_SUPPRESSED != 0 {
        return;
    }
    if !gc_budgeted_cycle_active() && flags & GC_FLAG_IN_ALLOC != 0 {
        return;
    }
    if gc_blocked_by_unsafe_zone() {
        return;
    }
    if defer_gc_request(DeferredGcRequest::CheckTrigger) {
        return;
    }

    // #5476: a workload that churns *large* temporaries (>16 KB, born directly
    // in the old arena) grows the old generation without ever exercising the
    // nursery. Old-gen reclaim pressure schedules a budgeted full cycle that
    // *would* return the dead old blocks to the OS — but the budgeted stepper is
    // blocked whenever synchronous-only root scanners are registered (the common
    // case in a compiled program), and even when it runs it only advances through
    // bounded mutator-assist steps that a compute-only loop never drives to
    // completion (no event-loop safepoint ever runs). Either way no collection
    // completes and RSS climbs unbounded. When old-gen reclaim pressure is what's
    // due — a rare event, gated by the ~32 MB growth / 48 MB absolute baseline, so
    // this never fires on the common nursery-churn path — run a direct full
    // mark-sweep to completion here, the same non-budgeted collection an explicit
    // `gc()` performs. The conservative native-stack scan (`force_full_scan`)
    // keeps it safe: anything still referenced from the stack/registers at this
    // allocation point (e.g. the temporary currently being built) is retained;
    // only genuinely unreachable old blocks are returned.
    if !gc_budgeted_cycle_active()
        && matches!(
            gc_budgeted_due_trigger(),
            Some(BudgetedGcTrigger::OldReclaim)
        )
        && !GC_OLD_RECLAIM_IN_PROGRESS.with(Cell::get)
    {
        let _reentry = OldReclaimReentryGuard::enter();
        GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
        let _scan = super::roots::ManualGcScanGuard::force_full_scan();
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(
            GcTriggerKind::OldGenBytes,
        ))
        .emit_after_current();
        return;
    }

    // The NURSERY-churn triggers (ArenaBytes / MallocCount) have the same
    // hole #5476 patched for OldReclaim: whenever synchronous-only root
    // scanners are registered — every compiled program, since codegen
    // registers sync scanners at startup — `gc_budgeted_start_blocked()`
    // holds for the life of the process, the mutator-assist step below can
    // never START a cycle, and allocation pressure accumulates without
    // bound (probe: 4M small allocations → 1.9 GB RSS with ZERO collection
    // cycles; the 64 MB arena trigger was due after the first ~64 blocks
    // and simply never fired). When the budgeted machinery is structurally
    // unavailable, run the direct synchronous minor the pre-budgeted
    // block-alloc trigger used to run. `gc_collect_minor_with_trigger`
    // re-baselines `GC_NEXT_TRIGGER_BYTES` (and, when it sweeps malloc,
    // the malloc trigger) on completion, and carries its own re-entrancy
    // guard (GC_FLAG_IN_ALLOC). `force_full_scan` mirrors the OldReclaim
    // arm: at an arbitrary allocation point a value mid-construction may
    // live only in registers, so the conservative native scan retains it —
    // which also makes copied-minor ineligible for THIS cycle, so the
    // non-moving minor runs (no relocation hazards at alloc points).
    if !gc_budgeted_cycle_active() && super::roots::registered_root_scanners_block_budgeted_gc() {
        let direct_kind = match gc_budgeted_due_trigger() {
            Some(BudgetedGcTrigger::ArenaBytes) => Some(GcTriggerKind::ArenaBytes),
            Some(BudgetedGcTrigger::MallocCount) => Some(GcTriggerKind::MallocCount),
            _ => None,
        };
        if let Some(kind) = direct_kind {
            let _scan = super::roots::ManualGcScanGuard::force_full_scan();
            super::gc_collect_minor_with_trigger(GcTriggerSnapshot::capture(kind))
                .emit_after_current();
            return;
        }
    }

    if !gc_budgeted_cycle_active() && gc_budgeted_due_trigger().is_none() {
        return;
    }

    let _ = gc_mutator_assist_step_work_units_inner_with_progress(
        GC_MUTATOR_ASSIST_WORK_UNITS,
        GcProgressKind::MutatorAssist,
    );
}

pub const JS_GC_STEP_STATUS_IDLE: u32 = 0;
pub const JS_GC_STEP_STATUS_ACTIVE: u32 = 1;
pub const JS_GC_STEP_STATUS_COMPLETED: u32 = 2;
pub const JS_GC_STEP_STATUS_SKIPPED: u32 = 3;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct JsGcStepResult {
    pub status: u32,
    pub phase: u32,
    pub collection_kind: u32,
    pub trigger_kind: u32,
    pub active: u32,
    pub completed: u32,
    pub arena_debt_bytes: u64,
    pub malloc_debt_objects: u64,
    pub old_reclaim_debt_bytes: u64,
}

#[derive(Clone, Copy)]
enum BudgetedGcRebaseline {
    ArenaBytes { pre_in_use: usize },
    MallocCount { pre_count: usize },
    OldReclaim,
}

struct BudgetedGcCycle {
    state: GcCycleState,
    trigger_kind: GcTriggerKind,
    collection_kind: GcCollectionKind,
    rebaseline: BudgetedGcRebaseline,
}

#[derive(Clone, Copy, Debug)]
enum BudgetedGcTrigger {
    OldReclaim,
    ArenaBytes,
    MallocCount,
}

thread_local! {
    static GC_BUDGETED_CYCLE: RefCell<Option<BudgetedGcCycle>> = const { RefCell::new(None) };
    static GC_BUDGETED_CYCLE_ACTIVE: Cell<bool> = const { Cell::new(false) };
    static GC_BUDGETED_STEP_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

pub(super) fn gc_budgeted_cycle_active() -> bool {
    GC_BUDGETED_CYCLE_ACTIVE.with(Cell::get)
}

fn gc_budgeted_start_blocked() -> bool {
    GC_FLAGS.with(|f| f.get()) & (GC_FLAG_IN_ALLOC | GC_FLAG_SUPPRESSED) != 0
        || gc_blocked_by_unsafe_zone()
        || GC_ROOT_LOCK_DEPTH.with(|depth| depth.get() != 0)
        || registered_root_scanners_block_budgeted_gc()
}

fn gc_budgeted_resume_blocked() -> bool {
    GC_FLAGS.with(|f| f.get()) & GC_FLAG_SUPPRESSED != 0
        || gc_blocked_by_unsafe_zone()
        || GC_ROOT_LOCK_DEPTH.with(|depth| depth.get() != 0)
        || registered_root_scanners_block_budgeted_gc()
}

pub(super) fn gc_old_reclaim_debt_bytes(old_in_use: usize, baseline: usize) -> u64 {
    let trigger = if baseline < GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES {
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES
    } else {
        baseline.saturating_add(GC_OLD_GEN_RECLAIM_GROWTH_BYTES)
    };
    old_in_use.saturating_sub(trigger) as u64
}

fn gc_budgeted_due_trigger() -> Option<BudgetedGcTrigger> {
    let old_pending = GC_OLD_RECLAIM_PENDING.with(Cell::get);
    // #6010: external Map/Set side-buffer bytes escalate to OldReclaim too.
    let old_in_use =
        crate::arena::old_gen_in_use_bytes().saturating_add(external_side_live_bytes());
    let old_baseline = GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.get());
    if old_pending || old_reclaim_pressure_due(old_in_use, old_baseline) {
        return Some(BudgetedGcTrigger::OldReclaim);
    }

    let total = crate::arena::arena_total_bytes();
    let next_arena_trigger = GC_NEXT_TRIGGER_BYTES.with(|c| c.get());
    if total >= next_arena_trigger {
        return Some(BudgetedGcTrigger::ArenaBytes);
    }

    let malloc_count = malloc_object_count();
    let next_malloc_trigger = GC_NEXT_MALLOC_TRIGGER.with(|c| c.get());
    if malloc_count >= next_malloc_trigger {
        return Some(BudgetedGcTrigger::MallocCount);
    }

    None
}

struct BudgetedGcStepGuard;

impl BudgetedGcStepGuard {
    fn enter() -> Option<Self> {
        GC_BUDGETED_STEP_ACTIVE.with(|active| {
            if active.get() {
                None
            } else {
                active.set(true);
                Some(Self)
            }
        })
    }
}

impl Drop for BudgetedGcStepGuard {
    fn drop(&mut self) {
        GC_BUDGETED_STEP_ACTIVE.with(|active| active.set(false));
    }
}

fn gc_start_budgeted_full_cycle(
    trigger_kind: GcTriggerKind,
    rebaseline: BudgetedGcRebaseline,
    progress_kind: GcProgressKind,
) -> BudgetedGcCycle {
    let mut state = GcCycleState::new_full(GcTriggerSnapshot::capture(trigger_kind));
    state.set_progress_kind(progress_kind);
    BudgetedGcCycle {
        collection_kind: state.collection_kind(),
        state,
        trigger_kind,
        rebaseline,
    }
}

fn gc_start_budgeted_minor_fallback_cycle(
    trigger_kind: GcTriggerKind,
    rebaseline: BudgetedGcRebaseline,
    progress_kind: GcProgressKind,
) -> BudgetedGcCycle {
    gc_start_budgeted_minor_fallback_cycle_with_snapshot(
        GcTriggerSnapshot::capture(trigger_kind),
        rebaseline,
        progress_kind,
    )
}

fn gc_start_budgeted_minor_fallback_cycle_with_snapshot(
    trigger: GcTriggerSnapshot,
    rebaseline: BudgetedGcRebaseline,
    progress_kind: GcProgressKind,
) -> BudgetedGcCycle {
    let prev_in_alloc = GC_FLAGS.with(|f| {
        let prev = f.get();
        f.set(prev | GC_FLAG_IN_ALLOC);
        prev & GC_FLAG_IN_ALLOC
    });
    let mut trace = GcCycleTrace::new(GcCollectionKind::Minor, trigger);
    if let Some(trace) = trace.as_mut() {
        trace.progress_kind = progress_kind;
    }
    let start = Instant::now();
    crate::arena::old_pages_begin_gc_cycle();
    clear_mark_seeds();
    let previous_pause_us = gc_last_pause_us();
    let current_rss_bytes = crate::process::get_rss_bytes();
    let low_pause_non_moving = progress_kind.is_budgeted();
    let evacuation_policy_allowed = !low_pause_non_moving && gen_gc_evacuate_enabled();
    let force_evacuation = !low_pause_non_moving && gc_force_evacuate_enabled();
    let evacuation_policy_disabled_reason = if low_pause_non_moving {
        EVACUATION_POLICY_LOW_PAUSE_NON_MOVING_REASON
    } else {
        EVACUATION_POLICY_DISABLED_REASON
    };
    let old_page_selection = if evacuation_policy_allowed && old_to_young_tracking_complete() {
        select_old_page_defrag_pages(force_evacuation)
    } else {
        OldPageDefragSelection::default()
    };
    let old_page_source_blocks =
        crate::arena::old_arena_source_blocks_for_pages(&old_page_selection.pages);
    let state = GcCycleState::new_minor_fallback(
        trigger,
        trace,
        start,
        progress_kind,
        prev_in_alloc,
        previous_pause_us,
        current_rss_bytes,
        evacuation_policy_allowed,
        force_evacuation,
        evacuation_policy_disabled_reason,
        old_page_selection,
        old_page_source_blocks,
    );
    BudgetedGcCycle {
        collection_kind: state.collection_kind(),
        state,
        trigger_kind: trigger.kind,
        rebaseline,
    }
}

#[cfg(test)]
pub(super) fn test_start_budgeted_minor_fallback_state_with_trace(
    trigger_kind: GcTriggerKind,
    progress_kind: GcProgressKind,
) -> GcCycleState {
    let trigger = GcTriggerSnapshot {
        kind: trigger_kind,
        steps_before: Some(GcStepSnapshot::current()),
    };
    let cycle = gc_start_budgeted_minor_fallback_cycle_with_snapshot(
        trigger,
        BudgetedGcRebaseline::ArenaBytes {
            pre_in_use: crate::arena::arena_in_use_bytes(),
        },
        progress_kind,
    );
    cycle.state
}

fn gc_start_budgeted_cycle_for_pressure(progress_kind: GcProgressKind) -> Option<BudgetedGcCycle> {
    let trigger = gc_budgeted_due_trigger()?;
    GC_TRIGGER_BUMPED.with(|c| c.set(false));
    Some(match trigger {
        BudgetedGcTrigger::OldReclaim => {
            GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
            gc_start_budgeted_full_cycle(
                GcTriggerKind::OldGenBytes,
                BudgetedGcRebaseline::OldReclaim,
                progress_kind,
            )
        }
        BudgetedGcTrigger::ArenaBytes => {
            let rebaseline = BudgetedGcRebaseline::ArenaBytes {
                pre_in_use: crate::arena::arena_in_use_bytes(),
            };
            if gen_gc_enabled() {
                gc_start_budgeted_minor_fallback_cycle(
                    GcTriggerKind::ArenaBytes,
                    rebaseline,
                    progress_kind,
                )
            } else {
                gc_start_budgeted_full_cycle(GcTriggerKind::ArenaBytes, rebaseline, progress_kind)
            }
        }
        BudgetedGcTrigger::MallocCount => {
            let rebaseline = BudgetedGcRebaseline::MallocCount {
                pre_count: malloc_object_count(),
            };
            if gen_gc_enabled() {
                gc_start_budgeted_minor_fallback_cycle(
                    GcTriggerKind::MallocCount,
                    rebaseline,
                    progress_kind,
                )
            } else {
                gc_start_budgeted_full_cycle(GcTriggerKind::MallocCount, rebaseline, progress_kind)
            }
        }
    })
}

fn gc_step_result(
    status: u32,
    phase: u32,
    collection_kind: u32,
    trigger_kind: u32,
    active: bool,
    completed: bool,
) -> JsGcStepResult {
    let debt = GcDebtSnapshot::current();
    JsGcStepResult {
        status,
        phase,
        collection_kind,
        trigger_kind,
        active: u32::from(active),
        completed: u32::from(completed),
        arena_debt_bytes: debt.arena_debt_bytes,
        malloc_debt_objects: debt.malloc_debt_objects,
        old_reclaim_debt_bytes: debt.old_reclaim_debt_bytes,
    }
}

fn gc_idle_step_result() -> JsGcStepResult {
    gc_step_result(JS_GC_STEP_STATUS_IDLE, 0, 0, 0, false, false)
}

fn gc_cycle_step_result(status: u32, cycle: &BudgetedGcCycle, completed: bool) -> JsGcStepResult {
    gc_step_result(
        status,
        cycle.state.phase().ffi_code(),
        cycle.collection_kind.ffi_code(),
        cycle.trigger_kind.ffi_code(),
        !completed,
        completed,
    )
}

fn gc_budgeted_status_result() -> JsGcStepResult {
    if !gc_budgeted_cycle_active() {
        return gc_idle_step_result();
    }

    let result = GC_BUDGETED_CYCLE.with(|slot| {
        let slot = slot.borrow();
        slot.as_ref()
            .map(|cycle| gc_cycle_step_result(JS_GC_STEP_STATUS_ACTIVE, cycle, false))
    });
    match result {
        Some(result) => result,
        None => {
            GC_BUDGETED_CYCLE_ACTIVE.with(|active| active.set(false));
            gc_idle_step_result()
        }
    }
}

fn gc_budgeted_skipped_result() -> JsGcStepResult {
    if !gc_budgeted_cycle_active() {
        return gc_step_result(JS_GC_STEP_STATUS_SKIPPED, 0, 0, 0, false, false);
    }

    GC_BUDGETED_CYCLE.with(|slot| {
        let slot = slot.borrow();
        slot.as_ref()
            .map(|cycle| gc_cycle_step_result(JS_GC_STEP_STATUS_SKIPPED, cycle, false))
            .unwrap_or_else(|| gc_step_result(JS_GC_STEP_STATUS_SKIPPED, 0, 0, 0, false, false))
    })
}

fn gc_finish_budgeted_cycle(mut cycle: BudgetedGcCycle) -> JsGcStepResult {
    let outcome = cycle
        .state
        .take_outcome()
        .expect("completed budgeted GC cycle must produce an outcome");
    match cycle.rebaseline {
        BudgetedGcRebaseline::ArenaBytes { pre_in_use } => {
            gc_finish_arena_trigger_collection(pre_in_use, outcome);
        }
        BudgetedGcRebaseline::MallocCount { pre_count } => {
            gc_finish_malloc_trigger_collection(pre_count, outcome);
        }
        BudgetedGcRebaseline::OldReclaim => {
            outcome.emit_after_current();
        }
    }
    GC_BUDGETED_CYCLE_ACTIVE.with(|active| active.set(false));
    gc_step_result(
        JS_GC_STEP_STATUS_COMPLETED,
        GcCyclePhase::Complete.ffi_code(),
        cycle.collection_kind.ffi_code(),
        cycle.trigger_kind.ffi_code(),
        false,
        true,
    )
}

enum BudgetedStepOutcome {
    Result(JsGcStepResult),
    Completed(BudgetedGcCycle),
}

fn gc_budgeted_step_work_units_inner(work_units: usize) -> JsGcStepResult {
    gc_budgeted_step_work_units_inner_with_progress(
        work_units,
        GcProgressKind::NormalIncremental,
        false,
    )
}

fn gc_budgeted_step_work_units_inner_with_progress(
    work_units: usize,
    start_progress_kind: GcProgressKind,
    mutator_assist_only: bool,
) -> JsGcStepResult {
    if work_units == 0 {
        return gc_budgeted_status_result();
    }

    let Some(_guard) = BudgetedGcStepGuard::enter() else {
        return gc_budgeted_skipped_result();
    };

    if !gc_budgeted_cycle_active() {
        if gc_budgeted_due_trigger().is_none() {
            return gc_idle_step_result();
        }
        if gc_budgeted_start_blocked() {
            return gc_budgeted_skipped_result();
        }
        let cycle = gc_start_budgeted_cycle_for_pressure(start_progress_kind)
            .expect("budgeted GC pressure was observed before starting cycle");
        GC_BUDGETED_CYCLE.with(|slot| {
            *slot.borrow_mut() = Some(cycle);
        });
        GC_BUDGETED_CYCLE_ACTIVE.with(|active| active.set(true));
    }

    if gc_budgeted_resume_blocked() {
        return gc_budgeted_skipped_result();
    }

    let outcome = GC_BUDGETED_CYCLE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(cycle) = slot.as_mut() else {
            GC_BUDGETED_CYCLE_ACTIVE.with(|active| active.set(false));
            return BudgetedStepOutcome::Result(gc_idle_step_result());
        };

        if mutator_assist_only && !cycle.state.phase().mutator_assist_honors_budget() {
            return BudgetedStepOutcome::Result(gc_cycle_step_result(
                JS_GC_STEP_STATUS_ACTIVE,
                cycle,
                false,
            ));
        }

        let step = cycle.state.step(GcWorkBudget::bounded(work_units));
        if step.completed {
            BudgetedStepOutcome::Completed(slot.take().expect("active budgeted GC cycle exists"))
        } else {
            BudgetedStepOutcome::Result(gc_cycle_step_result(
                JS_GC_STEP_STATUS_ACTIVE,
                cycle,
                false,
            ))
        }
    });

    match outcome {
        BudgetedStepOutcome::Result(result) => result,
        BudgetedStepOutcome::Completed(cycle) => gc_finish_budgeted_cycle(cycle),
    }
}

fn gc_mutator_assist_step_work_units_inner_with_progress(
    work_units: usize,
    start_progress_kind: GcProgressKind,
) -> JsGcStepResult {
    gc_budgeted_step_work_units_inner_with_progress(work_units, start_progress_kind, true)
}

pub(crate) fn gc_runtime_safepoint() -> JsGcStepResult {
    let budget = gc_progress_contract().budget_for(GcProgressKind::NormalIncremental);
    let Some(work_units) = budget.work_units else {
        return gc_budgeted_status_result();
    };
    gc_budgeted_step_work_units_inner_with_progress(
        work_units,
        GcProgressKind::NormalIncremental,
        false,
    )
}

fn write_gc_step_result(out: *mut JsGcStepResult, result: JsGcStepResult) -> u32 {
    if !out.is_null() {
        unsafe {
            *out = result;
        }
    }
    result.status
}

#[no_mangle]
pub extern "C" fn js_gc_step_work_units(work_units: u64, out: *mut JsGcStepResult) -> u32 {
    let work_units = usize::try_from(work_units).unwrap_or(usize::MAX);
    let result = gc_budgeted_step_work_units_inner(work_units);
    write_gc_step_result(out, result)
}

#[no_mangle]
pub extern "C" fn js_gc_step_us(budget_us: u64, out: *mut JsGcStepResult) -> u32 {
    if budget_us == 0 {
        let result = gc_budgeted_status_result();
        return write_gc_step_result(out, result);
    }

    let start = Instant::now();
    let mut result = gc_budgeted_step_work_units_inner(1);
    while result.status == JS_GC_STEP_STATUS_ACTIVE
        && start.elapsed().as_micros() < u128::from(budget_us)
    {
        result = gc_budgeted_step_work_units_inner(1);
    }
    write_gc_step_result(out, result)
}

#[no_mangle]
pub extern "C" fn js_gc_step_status(out: *mut JsGcStepResult) -> u32 {
    let result = gc_budgeted_status_result();
    write_gc_step_result(out, result)
}

#[no_mangle]
pub extern "C" fn js_gc_safepoint(out: *mut JsGcStepResult) -> u32 {
    let result = gc_runtime_safepoint();
    write_gc_step_result(out, result)
}

/// Counter tracking "native work holds JSValue roots we can't scan" state.
/// This is for narrow FFI sections where a worker thread may temporarily
/// hold runtime values on a stack the main-thread GC cannot see. Long-lived
/// server adapters should instead queue plain Rust data, allocate JS values
/// on the main thread, and register mutable root scanners for stored callback
/// slots.
///
/// When > 0, the conservative main-thread stack scanner can't see all live
/// roots — collecting could free objects still referenced from worker-thread
/// stacks and SEGV on next access.
///
/// Issue #31: gc() from setInterval in a Fastify+WebSocket server crashed
/// within 60s of the first tick because WS worker threads held live refs
/// to message payload strings on their stacks. This counter lets stdlib
/// features signal "please skip user-initiated gc() while I'm running"
/// without a full stop-the-world mutex.
pub static GC_UNSAFE_ZONES: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// One-shot warning so we don't spam stderr on every tick.
pub(super) static GC_UNSAFE_WARNED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Manual GC trigger (callable from TypeScript as `gc()`). Skipped when
/// worker threads are active (see GC_UNSAFE_ZONES).
#[no_mangle]
pub extern "C" fn js_gc_collect() {
    if manual_gc_blocked_by_unsafe_zone() {
        return;
    }
    if defer_gc_request(DeferredGcRequest::Collect(GcTriggerKind::Manual)) {
        return;
    }
    manual_gc_collect_now();
}

/// Run an explicit (`gc()`) full collection. The `gc()` callsite may hold live
/// module-init/top-level locals only on the native stack, so the collection
/// forces the conservative native-stack scan (#4977); see `ManualGcScanGuard`.
fn manual_gc_collect_now() {
    let _scan = super::roots::ManualGcScanGuard::force_full_scan();
    crate::weakref::clear_pending_finalization_jobs();
    // An explicit `gc()` runs a FULL mark-sweep rather than the generational
    // fast path. With gen-GC on (the default), `gc_collect_inner_with_trigger`
    // dispatches a MINOR cycle, whose sweep skips dead-old-block reclamation
    // (`reclaim_dead_old_blocks = false`) — so dead large/tenured objects (which
    // are born in the old arena, >16 KB) survived an explicit `gc()` and RSS
    // never dropped. A full cycle reclaims them, matching V8/Node `--expose-gc`
    // semantics where `gc()` is a full collection. Automatic threshold-driven
    // minors are unaffected.
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Manual))
        .emit_after_current();
    crate::weakref::queue_pending_finalization_callbacks_after_gc();
}

pub(super) fn gc_blocked_by_unsafe_zone() -> bool {
    GC_UNSAFE_ZONES.load(std::sync::atomic::Ordering::Acquire) > 0
}

pub(super) fn manual_gc_blocked_by_unsafe_zone() -> bool {
    if gc_blocked_by_unsafe_zone() {
        unsafe_zone_manual_gc_warning();
        return true;
    }
    false
}

pub(super) fn unsafe_zone_manual_gc_warning() {
    if !GC_UNSAFE_WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        // One-shot warning — user likely has `setInterval(() => gc(), N)`
        // in a server; we don't want to print every 30s.
        eprintln!(
            "perry: gc() skipped — native work may hold JSValue refs on a \
             worker thread that the main-thread GC can't see. Manual gc() \
             is a no-op until that unsafe work exits."
        );
    }
}

/// Increment GC_UNSAFE_ZONES for a narrow FFI section whose worker thread may
/// hold JSValue roots the main-thread scanner cannot see.
#[no_mangle]
pub extern "C" fn js_gc_enter_unsafe_zone() {
    GC_UNSAFE_ZONES.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
}

/// Decrement GC_UNSAFE_ZONES when the matching unsafe FFI section exits.
#[no_mangle]
pub extern "C" fn js_gc_exit_unsafe_zone() {
    GC_UNSAFE_ZONES.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
}

/// Threshold-based GC trigger (safe for use from the event loop).
/// Only runs collection if arena or malloc thresholds are exceeded.
#[no_mangle]
pub extern "C" fn gc_check_trigger_export() {
    gc_check_trigger();
}
