//! #6185 Tier 2 — owner-tagged cross-thread dispatch.
//!
//! These exercise the three process-global queues the 2026-07-09 GC audit
//! flagged (`PENDING_THREAD_RESULTS`, `TIMER_QUEUE` / `CALLBACK_TIMERS` /
//! `INTERVAL_TIMERS`) at the Rust level: an entry enqueued by one agent must be
//! invisible to every other agent's drain, and must survive — not be silently
//! eaten — when a foreign agent pumps.
//!
//! The end-to-end JS behavior (a worker's `await` no longer stealing the main
//! thread's `spawn` completion) rides on top of this and is covered by the
//! compiled integration test in
//! `crates/perry/tests/issue_6185_thread_owner_tagged_dispatch.rs`.

use crate::agent::{current_agent, enter_worker_agent, owns, retire_agent, PRIMARY_AGENT};

/// The timer queues are process-global, and `cargo test` runs these in parallel
/// threads of one process — so two tests that both schedule on the PRIMARY agent
/// would see each other's timers in `active_timeout_resource_count()`. Serialize
/// the ones that assert on primary-agent counts.
static TIMER_QUEUE_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The Android invariant, stated as a test: `nativePumpTick` runs on the UI
/// thread, which never claims a worker agent, so it must resolve to the same
/// agent as the `perry-native` thread that scheduled the timers. If this ever
/// regresses to a raw-`ThreadId` comparison, every Android timer stops firing —
/// the exact blocker that deferred this fix the first time round.
#[test]
fn a_pump_thread_shares_the_primary_agent_with_the_js_thread() {
    // The "perry-native" thread: runs JS, never claims a worker agent.
    let js_thread = std::thread::spawn(current_agent).join().unwrap();
    // The UI thread: pure pump, also never claims a worker agent.
    let ui_pump_thread = std::thread::spawn(current_agent).join().unwrap();

    assert_eq!(js_thread, PRIMARY_AGENT);
    assert_eq!(
        ui_pump_thread, js_thread,
        "the UI pump must own the JS thread's work, or Android timers never fire"
    );
    // And it can in fact claim that work.
    assert!(std::thread::spawn(|| owns(PRIMARY_AGENT)).join().unwrap());
}

/// A worker owns its own agent and disowns the primary agent's work — this is
/// what stops a worker's await loop from draining the main thread's queues.
#[test]
fn a_worker_disowns_the_primary_agents_work() {
    let (worker_agent, worker_owns_primary) = std::thread::spawn(|| {
        let id = enter_worker_agent();
        let owns_primary = owns(PRIMARY_AGENT);
        retire_agent(id);
        (id, owns_primary)
    })
    .join()
    .unwrap();

    assert_ne!(worker_agent, PRIMARY_AGENT);
    assert!(
        !worker_owns_primary,
        "a worker draining primary-agent work is the #6185 use-after-free"
    );
}

/// A timer scheduled by the primary agent must not be *fired* by a worker — and
/// must still be there afterwards for its real owner. The pre-fix drain would
/// have run the callback on the worker (cross-heap call) or, with a naive
/// filter-and-drop, silently eaten it.
#[test]
fn a_worker_neither_fires_nor_eats_a_primary_agent_timer() {
    let _serial = TIMER_QUEUE_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    // Schedule on the primary agent (this test thread).
    let before = crate::timer::active_timeout_resource_count();
    let id = crate::timer::js_set_timeout_callback(0, 50_000.0);
    assert_eq!(
        crate::timer::active_timeout_resource_count(),
        before + 1,
        "the owning agent sees its own timer as an active handle"
    );

    // A worker pumps the timer queues. It must fire nothing of ours...
    let fired_on_worker = std::thread::spawn(|| {
        let agent = enter_worker_agent();
        let fired = crate::timer::js_callback_timer_tick() + crate::timer::js_interval_timer_tick();
        // ...and must not count our timer as work keeping ITS loop alive.
        let visible = crate::timer::active_timeout_resource_count();
        let has_pending = crate::timer::js_callback_timer_has_pending();
        retire_agent(agent);
        (fired, visible, has_pending)
    })
    .join()
    .unwrap();

    assert_eq!(fired_on_worker.0, 0, "a worker must not fire our timers");
    assert_eq!(
        fired_on_worker.1, 0,
        "a worker must not see our timer as its active handle"
    );
    assert_eq!(
        fired_on_worker.2, 0,
        "a foreign timer must not keep a worker's event loop alive"
    );

    // The timer is still ours, untouched, after the foreign pump.
    assert_eq!(
        crate::timer::active_timeout_resource_count(),
        before + 1,
        "a foreign pump must not consume the owner's timer"
    );

    crate::timer::clearTimeout(id);
}

/// The GC root scanner must walk only the collecting agent's own timers.
///
/// This is the subtlest half of the fix. `scan_timer_roots_mut` runs on every GC
/// cycle and, on an *evacuating* cycle, REWRITES the slots it visits to
/// forwarding addresses in the collecting thread's arena. Walking a foreign
/// agent's timer there would mark through a heap this collector does not own
/// (racing that agent's collector on the same `GcHeader` bits) and rewrite its
/// promise/closure slots to point into the wrong arena.
///
/// So a worker collecting while the primary agent holds a timer must visit zero
/// of the primary agent's slots — while the owner still roots its own.
#[test]
fn a_workers_gc_scan_never_visits_another_agents_timer_slots() {
    let _serial = TIMER_QUEUE_TESTS.lock().unwrap_or_else(|e| e.into_inner());

    // A promise timer on the primary agent. Its `promise` (a raw pointer into
    // THIS arena) and `value` slots are scanned unconditionally, which is what
    // makes it a clean probe for "did the collector touch a foreign slot?".
    let baseline = {
        let mut n = 0usize;
        crate::timer::scan_timer_roots(&mut |_v: f64| n += 1);
        n
    };
    let _promise = crate::timer::js_set_timeout(50_000.0);
    let visited_by_owner = {
        let mut n = 0usize;
        crate::timer::scan_timer_roots(&mut |_v: f64| n += 1);
        n
    };
    assert!(
        visited_by_owner > baseline,
        "the owning agent must root its own timer's slots, or the promise is swept"
    );

    // The same scan on a worker must not see any of it.
    let visited_on_worker = std::thread::spawn(|| {
        let agent = enter_worker_agent();
        let mut visited = 0usize;
        crate::timer::scan_timer_roots(&mut |_v: f64| visited += 1);
        retire_agent(agent);
        visited
    })
    .join()
    .unwrap();

    assert_eq!(
        visited_on_worker, 0,
        "a worker's GC must not mark (or evacuate-rewrite) the primary agent's timer slots"
    );

    crate::timer::test_clear_all_timer_scanner_roots();
}

/// Retiring a worker purges the timers it scheduled: its arena is unmapped, so
/// nothing can ever legally run those callbacks. Pre-fix they stayed on the
/// global queue and fired on whatever thread pumped next — reading freed memory.
#[test]
fn retiring_a_worker_purges_the_timers_it_scheduled() {
    let _serial = TIMER_QUEUE_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    let before = crate::timer::active_timeout_resource_count();

    std::thread::spawn(|| {
        let agent = enter_worker_agent();
        // A worker scheduling a timer: the callback closure would live in this
        // worker's arena.
        crate::timer::js_set_timeout_callback(0, 60_000.0);
        crate::timer::setInterval(0, 60_000.0);
        assert!(
            crate::timer::active_timeout_resource_count() >= 2,
            "the worker sees its own timers while it is alive"
        );
        retire_agent(agent);
        assert_eq!(
            crate::timer::active_timeout_resource_count(),
            0,
            "retiring the agent drops the timers it owned"
        );
        assert!(
            !owns(PRIMARY_AGENT),
            "a retired worker must not fall back to owning the primary agent's work"
        );
    })
    .join()
    .unwrap();

    assert_eq!(
        crate::timer::active_timeout_resource_count(),
        before,
        "a dead worker's timers must not linger on the primary agent's queue"
    );
}
