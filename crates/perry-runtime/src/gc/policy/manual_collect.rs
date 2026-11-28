//! Explicit collection entries (`js_gc_collect`, the `gc` module) and the
//! native unsafe-zone gate they honor.

use super::*;

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
pub(in crate::gc) static GC_UNSAFE_WARNED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Manual GC trigger (callable from TypeScript as `gc()`). Skipped when
/// worker threads are active (see GC_UNSAFE_ZONES).
#[cfg(not(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu")))]
#[no_mangle]
pub extern "C" fn js_gc_collect() {
    js_gc_collect_body();
}

/// x86-64 Linux: `gc()` hands its caller's frame to the collections it runs,
/// exactly as the back-edge poll does (see `js_gc_loop_safepoint`).
#[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"))]
#[unsafe(naked)]
#[no_mangle]
pub extern "C" fn js_gc_collect() {
    super::offering_entry_asm!(0, js_gc_collect_from)
}

#[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"))]
extern "C" fn js_gc_collect_from(pc: usize, sp: usize, fp: usize) {
    let origin = super::WalkOrigin { pc, sp, fp };
    let _origin = super::EntryWalkOrigin::enter(&origin);
    js_gc_collect_body();
}

fn js_gc_collect_body() {
    crate::snap_funnel!(Gc);
    if crate::snap_census::explicit_gc() {
        return;
    }
    if manual_gc_blocked_by_unsafe_zone() {
        return;
    }
    // `PERRY_GC_CENSUS`: an explicit gc() takes a census in the full cycle it
    // runs (or, if deferred/blocked, in the next full cycle). No-op otherwise.
    super::census::census_arm("manual");
    if defer_gc_request(DeferredGcRequest::Collect(GcTriggerKind::Manual)) {
        return;
    }
    let start = std::time::Instant::now();
    manual_gc_collect_now();
    super::instruments::note_full_collect_us(start.elapsed().as_micros() as u64);
    crate::snap_census::explicit_gc_done();
}

/// Run an explicit (`gc()`) full collection, with **precise roots** — the same
/// root set every automatic collection in a production binary already uses
/// (`conservative_stack_scan_mode()` resolves to `Auto`, i.e. `SkipDisabled`).
///
/// ★ #7558: this site used to take `ManualGcScanGuard::force_full_scan`. It no
/// longer does, and the removal is deliberate rather than incidental — read
/// this before adding one back.
///
/// **What the scan was for.** #4977: `const keep = {…}; gc(); keep.nested.deep`
/// read dangling-pointer garbage, because a module-init/top-level local was
/// held only as a native-stack alloca that neither the shadow stack nor the
/// module-var scanners covered. Forcing the conservative scan retained it. That
/// was a *workaround for a precise-rooting hole*, applied at the one collection
/// site that could be made to hide it — not a statement that `gc()` needs a
/// different root set from every other collection.
///
/// **Why it is no longer needed.** The hole was closed by the 2026-06→08
/// rooting campaign, from a different direction: pointer-typed locals get a
/// persistent shadow slot bound in the function-entry setup (#6968's
/// `expr::scalar_slot_root`, #6951/#6972's object-literal rooting), module-level
/// bindings are `@perry_global_*` cells registered with
/// `js_gc_register_global_root`, and `scripts/gc_root_dominance_check.py` gates
/// the invariant that a root store must dominate every collection point — with
/// an **empty** allowlist. `js_gc_collect` is a collection point by that
/// invariant like any other; nothing about it is special. #4977's own repro
/// (`tests/parity/language/class/test_issue_4977_gc_toplevel_locals.ts`) prints the right answer
/// with the scan disabled.
///
/// **What it cost.** A conservative scan retains whatever the native stack
/// happens to look like a pointer to, so the reading every retained-heap number
/// in this project is taken through — `process.memoryUsage()` after `gc()` —
/// carried a stack-residue tax. Measured with `gc_ratchet.py classify` on
/// `main` at `961777904`: non-zero on **nine of the twelve** ratchet probes,
/// and 28.63% / 28.24% / 29.71% / 31.03% of reported retention on
/// `01_nursery_churn`, `05_closure_capture`, `06_string_retention` and
/// `11_collect_at_depth` respectively (13.80%, 8,273,888 bytes, on
/// `12_large_live_set` — the case #7558 was filed for was the *smallest* of the
/// four). It was also non-deterministic run to run, because stack residue is,
/// which is why `benchmarks/gc_ratchet` had to stop gating that cell (#7554)
/// and why retention rows were unbelievable without a manual `classify`
/// cross-check (#7559). And it made this path non-moving, which is why
/// `PERRY_GC_FORCE_EVACUATE` was inert for every `gc()`-driven test
/// (#6942/#6946).
///
/// **A second-order effect worth knowing about.** `gc/tenuring.rs` deliberately
/// refuses to seed the adaptive tenuring threshold from a cycle that ran the
/// conservative scan, so on any `gc()`-driven workload that seed had never
/// fired. It fires now. On two ratchet probes `tenuring_survivals` falls
/// `4 -> 1` and survivors are promoted on first copy rather than copied into
/// survivor space; the copying minor still runs and moves *more* objects.
/// Removing a conservative scan is not only a retention change.
///
/// **What is unchanged.** `gc()` is still synchronous — #7148's disposition
/// that it must not be *deferred* to a safepoint stands, because
/// `gc(); assertFreed()` is the shape every test and every ratchet probe uses.
/// This changes the root set, not the timing.
pub(super) fn manual_gc_collect_now() {
    // NOTE: pending finalization jobs are NOT delivered here. A cleanup
    // callback is its own later job (spec: HostEnqueueFinalizationRegistry
    // CleanupJob), so until the next microtask-pump drain delivers it the
    // dead cell is still registered: `gc(); registry.unregister(token)` in the
    // same synchronous turn removes it and returns true, and its callback
    // never runs (node does the same).
    // An explicit `gc()` runs a FULL mark-sweep rather than the generational
    // fast path. With gen-GC on (the default), `gc_collect_inner_with_trigger`
    // dispatches a MINOR cycle, whose sweep skips dead-old-block reclamation
    // (`reclaim_dead_old_blocks = false`) — so dead large/tenured objects (which
    // are born in the old arena, >16 KB) survived an explicit `gc()` and RSS
    // never dropped. A full cycle reclaims them, matching V8/Node `--expose-gc`
    // semantics where `gc()` is a full collection. Automatic threshold-driven
    // minors are unaffected.
    //
    // ★ #6946: under forced evacuation, run an EVACUATING minor FIRST.
    //
    // `PERRY_GC_FORCE_EVACUATE` is read only on the minor path, so every test
    // of the shape `gc(); assertFreed()` under that knob looked like evacuation
    // stress coverage and was a full mark-sweep that moved nothing —
    // CLAUDE.md's hazard 4, and one of the three worked examples in it. Five
    // suites still drive collection exactly that way
    // (`gc_property_key_operand_rooting_6935`,
    // `gc_dynamic_arith_operand_rooting_6655`,
    // `gc_string_coerce_property_key_rooting_6943`,
    // `gc_side_table_roots_evacuation`, and `gc/tests/cycle_state.rs`).
    //
    // What made this impossible before and does not any more: this site used to
    // take `ManualGcScanGuard::force_full_scan`, and a forced conservative scan
    // makes the copying minor ineligible outright
    // (`CopiedMinorFallbackReason::ConservativeStack`). #7657 removed it — see
    // the doc comment above — so `gc()` now runs on precise roots and a copying
    // minor here is exactly as sound as the full mark-sweep below.
    //
    // `FullEscalation::Refused`, because the two throughput-pacing predicates
    // would hand this call a full sweep: a NON-MOVING collection under a knob
    // whose entire name is about relocation, which is the original bug in a new
    // place. The full sweep follows immediately anyway, so refusing the
    // escalation here costs nothing it was protecting.
    //
    // Default-off knob, so nothing about an ordinary `gc()` changes.
    if super::gc_force_evacuate_enabled() {
        super::gc_collect_forced_evacuating_minor(GcTriggerSnapshot::capture(
            GcTriggerKind::Manual,
        ))
        .emit_after_current();
    }
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Manual))
        .emit_after_current();
}

/// `perry/gc` `collect()` — explicit full collection, same semantics as the
/// global `gc()`. Returns `undefined` so the JS surface has a stable shape.
#[no_mangle]
pub extern "C" fn js_gc_module_collect() -> f64 {
    js_gc_collect();
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// `perry/gc` `minor()` — synchronous nursery-only collection; returns the
/// freed byte count (0 when skipped: unsafe zone or deferred). Like `gc()`,
/// the callsite may hold live locals only on the native stack, so force the
/// conservative scan (#4977).
///
/// ★ #7148 disposition: **keep, observable** — same reasoning as
/// `manual_gc_collect_now`. `minor()` returns the freed byte count, so its
/// result is only meaningful if the collection ran before it returned;
/// deferring would have to return 0 and silently mean something else.
#[no_mangle]
pub extern "C" fn js_gc_module_minor() -> f64 {
    if manual_gc_blocked_by_unsafe_zone() {
        return 0.0;
    }
    let _scan =
        super::roots::ManualGcScanGuard::force_full_scan(super::ConservativeScanSite::ManualMinor);
    super::gc_collect_minor() as f64
}

/// `perry/gc` `idleHint()` — frame-boundary pacing hint for latency-sensitive
/// programs (games, interactive UIs). If a threshold-driven collection is
/// already due, run it NOW — at a point the caller declared idle — instead of
/// letting it land mid-frame at an arbitrary allocation site. O(1) when
/// nothing is due. Returns whether a collection ran.
#[no_mangle]
pub extern "C" fn js_gc_module_idle_hint() -> f64 {
    let before = gc_total_collection_count();
    gc_check_trigger();
    let ran = gc_total_collection_count() != before;
    f64::from_bits(crate::value::JSValue::bool(ran).bits())
}

pub(in crate::gc) fn gc_blocked_by_unsafe_zone() -> bool {
    #[cfg(test)]
    if let Some(blocked) = unsafe_zone_test_override::OVERRIDE.with(std::cell::Cell::get) {
        return blocked;
    }
    GC_UNSAFE_ZONES.load(std::sync::atomic::Ordering::Acquire) > 0
}

/// Per-thread override of [`gc_blocked_by_unsafe_zone`] for the unit suite
/// (#7946).
///
/// [`GC_UNSAFE_ZONES`] is genuinely process-wide in production — the whole
/// point is that a *worker thread* holding JSValues on an unscannable stack
/// stops the main thread collecting. In a test binary that same property makes
/// `GC_UNSAFE_ZONES.store(1)` a global stop-the-collector: every concurrent
/// libtest thread's `gc_budgeted_start_blocked()` / `gc_budgeted_resume_
/// blocked()` starts answering "blocked", and any test driving a budgeted cycle
/// to completion gets `JS_GC_STEP_STATUS_SKIPPED` instead. That is exactly what
/// `gc::tests::root_words::bare_address_in_{shadow_slot,global_root}_survives_
/// a_real_collection` failed with ("budgeted GC cycle stopped before
/// completion: status 3"), 3 runs in 100.
///
/// The unsafe-zone tests are all single-threaded — they set the zone and then
/// assert about a collection on the same thread — so a per-thread pin tests the
/// same predicate without reaching outside the test.
#[cfg(test)]
pub(in crate::gc) mod unsafe_zone_test_override {
    use std::cell::Cell;

    thread_local! {
        pub(super) static OVERRIDE: Cell<Option<bool>> = const { Cell::new(None) };
    }

    /// Pin (or unpin) `gc_blocked_by_unsafe_zone()` for this thread, returning
    /// the previous pin so a guard can restore it.
    pub(crate) fn set_unsafe_zone_blocked_for_test(blocked: Option<bool>) -> Option<bool> {
        OVERRIDE.with(|c| c.replace(blocked))
    }
}

pub(in crate::gc) fn manual_gc_blocked_by_unsafe_zone() -> bool {
    if gc_blocked_by_unsafe_zone() {
        unsafe_zone_manual_gc_warning();
        return true;
    }
    false
}

pub(in crate::gc) fn unsafe_zone_manual_gc_warning() {
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
