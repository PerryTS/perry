//! turnloop P0 driver tests. Each asserts its subject ran — a turn happened,
//! an OS wait was counted, a wake syscall fired — not merely that nothing threw.

use super::*;
use std::sync::mpsc;
use std::time::Duration;

fn serial() -> std::sync::MutexGuard<'static, ()> {
    super::super::tests::SERIAL
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Give this test thread a loop WITHOUT the process-wide route, so a test that
/// only exercises the turn cannot race another thread for route ownership.
fn install_unrouted() {
    let agent = AgentLoop::new().expect("create agent loop");
    AGENT_LOOP.with(|slot| *slot.borrow_mut() = Some(agent));
    STATE.with(|s| s.set(LoopState::Owner));
}

fn stats() -> LoopStats {
    loop_statistics().expect("this thread owns a loop")
}

/// Claim the primary route on this thread, waiting out a route held by a test
/// thread that is still finishing.
fn claim_route() {
    let limit = Instant::now() + Duration::from_secs(10);
    loop {
        STATE.with(|s| s.set(LoopState::Unset));
        if ensure_loop() {
            return;
        }
        assert!(Instant::now() < limit, "primary route never became free");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn route_is_free() -> bool {
    PRIMARY_ROUTE
        .notifier
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_none()
}

/// DESIGN §10 rule 4a on the host side: with an idle registered socket and a
/// 0.5 ms, 2 ms or 10 ms deadline, the park ends at the deadline in at most
/// two turns with at most one zero-event OS wait — it waits, it never spins.
#[test]
fn sub_and_whole_millisecond_deadlines_wait_without_spinning() {
    let _g = serial();
    std::thread::spawn(|| {
        install_unrouted();
        let listener = AGENT_LOOP.with(|slot| {
            slot.borrow_mut()
                .as_mut()
                .unwrap()
                .driver
                .tcp_listen(
                    "127.0.0.1:0".parse().unwrap(),
                    &turnloop::ListenOpts::default(),
                )
                .expect("idle registered socket")
        });
        for micros in [500u64, 2_000, 10_000] {
            // A notify from an unrelated test thread would legitimately skip a
            // wait; retry for a clean window instead of reading that as a spin.
            let mut clean = false;
            for _attempt in 0..20 {
                super::super::NOTIFIED.store(false, Ordering::SeqCst);
                let before = stats();
                let start = Instant::now();
                let deadline = start + Duration::from_micros(micros);
                let mut interfered = false;
                let mut parks = 0u32;
                while Instant::now() < deadline {
                    parks += 1;
                    assert!(parks < 1_000, "{micros} us deadline spun: {parks} parks");
                    match park_until(deadline) {
                        Park::Waited => {}
                        Park::Notified => interfered = true,
                        Park::Failed => panic!("turn failed"),
                    }
                }
                if interfered {
                    continue;
                }
                let after = stats();
                let turns = after.turns - before.turns;
                let os_waits = after.os_waits - before.os_waits;
                let zero = after.zero_event_waits - before.zero_event_waits;
                assert!(turns >= 1, "{micros} us: no turn ran");
                assert!(turns <= 2, "{micros} us: {turns} turns");
                assert!(os_waits >= 1, "{micros} us: no OS wait ran");
                assert!(zero <= 1, "{micros} us: {zero} zero-event waits");
                assert!(
                    start.elapsed() >= Duration::from_micros(micros),
                    "{micros} us: returned before the deadline"
                );
                clean = true;
                break;
            }
            assert!(clean, "{micros} us: never observed an uninterrupted window");
        }
        AGENT_LOOP.with(|slot| {
            let mut slot = slot.borrow_mut();
            let agent = slot.as_mut().unwrap();
            agent.driver.close(listener, turnloop::Token(1)).unwrap();
            agent
                .driver
                .turn(Timeout::Now, &mut agent.completions)
                .unwrap();
            assert!(!agent.driver.alive(), "listener close did not settle");
        });
    })
    .join()
    .unwrap();
}

/// A producer on another thread wakes a parked primary loop through the real
/// `js_notify_main_thread` entry, with exactly one native wake syscall.
#[test]
fn another_thread_wakes_a_parked_turn_through_js_notify_main_thread() {
    let _g = serial();
    super::super::loop_stats::force_enable_for_test();
    let waits_before = super::super::loop_stats::snapshot();
    let (parked_tx, parked_rx) = mpsc::channel();
    let owner = std::thread::spawn(move || {
        claim_route();
        super::super::NOTIFIED.store(false, Ordering::SeqCst);
        let notifier = AGENT_LOOP.with(|slot| slot.borrow().as_ref().unwrap().driver.notifier());
        parked_tx.send(()).unwrap();
        let start = Instant::now();
        let park = park_until(start + Duration::from_secs(30));
        let waited = start.elapsed();
        let stats = stats();
        let syscalls = notifier.wake_syscalls();
        shutdown_current_thread();
        (park, waited, stats, syscalls)
    });
    parked_rx.recv().unwrap();
    let limit = Instant::now() + Duration::from_secs(10);
    // Wait until the owner is blocked in the OS wait itself, so the wake has to
    // take the syscall path rather than a pre-park notification bit.
    loop {
        let parked = PRIMARY_ROUTE.in_turn.load(Ordering::SeqCst)
            && PRIMARY_ROUTE
                .notifier
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
                .is_some_and(|(_, notifier)| notifier.is_parked());
        if parked {
            break;
        }
        assert!(Instant::now() < limit, "owner never parked in its turn");
        std::thread::yield_now();
    }
    super::super::js_notify_main_thread();
    let (park, waited, stats, syscalls) = owner.join().unwrap();
    assert_eq!(park, Park::Waited);
    assert!(
        waited < Duration::from_secs(10),
        "wake was lost: waited {waited:?}"
    );
    assert_eq!(stats.turns, 1, "{stats:?}");
    assert_eq!(stats.os_waits, 1, "{stats:?}");
    assert!(
        syscalls >= 1,
        "the cross-thread wake never reached the OS wait"
    );
    assert!(route_is_free(), "shutdown left the route installed");
    // PERRY_LOOP_STATS: exactly one turnloop wait and one wake-latency sample.
    let waits_after = super::super::loop_stats::snapshot();
    assert_eq!(waits_after.turnloop.count - waits_before.turnloop.count, 1);
    assert_eq!(
        waits_after.wake_samples() - waits_before.wake_samples(),
        1,
        "one cross-thread notify must produce exactly one wake-latency sample"
    );
    super::super::NOTIFIED.store(false, Ordering::SeqCst);
}

/// Install on first use, idempotent shutdown, no reinstall afterwards, and the
/// route is released both by shutdown and by plain thread exit.
#[test]
fn install_shutdown_and_thread_exit_release_the_loop_and_route() {
    let _g = serial();
    std::thread::spawn(|| {
        claim_route();
        assert!(eligible());
        assert!(loop_statistics().is_some());
        assert!(!route_is_free());
        shutdown_current_thread();
        shutdown_current_thread();
        assert!(loop_statistics().is_none(), "shutdown kept the loop");
        assert!(route_is_free(), "shutdown kept the route");
        assert!(!eligible(), "a shut-down thread must not reinstall");
        assert!(!ensure_loop());
    })
    .join()
    .unwrap();
    std::thread::spawn(claim_route).join().unwrap();
    assert!(route_is_free(), "thread exit kept the route");
}

/// Worker agents have no loop in P0 and keep the legacy park.
#[test]
fn worker_agents_are_declined() {
    std::thread::spawn(|| {
        let agent = crate::agent::enter_worker_agent();
        assert!(!eligible());
        assert!(!ensure_loop());
        assert!(loop_statistics().is_none());
        crate::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}

/// `fast()` turns only when turnloop has outstanding work: no OS call while
/// the loop is idle (the P0 steady state), one nonblocking turn once a
/// turnloop timer is armed.
#[test]
fn fast_turn_polls_only_with_outstanding_loop_work() {
    std::thread::spawn(|| {
        install_unrouted();
        fast_turn();
        assert_eq!(stats().turns, 0, "idle fast path made an OS call");
        let deadline = Instant::now() + Duration::from_secs(60);
        let timer = AGENT_LOOP.with(|slot| {
            let mut slot = slot.borrow_mut();
            let agent = slot.as_mut().unwrap();
            agent
                .driver
                .timer(deadline, None, turnloop::Token(7))
                .unwrap()
        });
        assert_eq!(loop_deadline(), Some(deadline), "loop deadline not visible");
        fast_turn();
        assert_eq!(stats().turns, 1, "fast path ignored outstanding loop work");
        AGENT_LOOP.with(|slot| {
            let mut slot = slot.borrow_mut();
            let agent = slot.as_mut().unwrap();
            agent.driver.close(timer, turnloop::Token(8)).unwrap();
            agent
                .driver
                .turn(Timeout::Now, &mut agent.completions)
                .unwrap();
        });
    })
    .join()
    .unwrap();
}

extern "C" fn native_work_in_flight() -> i32 {
    1
}

/// Work that becomes visible to the native in-flight predicate after the park
/// chose a turnloop wait is honoured before the OS wait (no lost wake).
#[test]
fn native_work_visible_before_the_turn_skips_the_wait() {
    let _g = serial();
    std::thread::spawn(|| {
        install_unrouted();
        super::super::NOTIFIED.store(false, Ordering::SeqCst);
        super::super::js_register_native_inflight(Some(native_work_in_flight));
        let start = Instant::now();
        let park = park_until(start + Duration::from_secs(30));
        super::super::js_register_native_inflight(None);
        assert_eq!(park, Park::Notified);
        assert!(start.elapsed() < Duration::from_secs(10));
        assert_eq!(stats().turns, 0, "the wait ran despite pending native work");
    })
    .join()
    .unwrap();
}

/// End to end through `js_wait_for_event`: a real 2 ms Perry timer is reached
/// by precise parks, not by a spin, and the loop that did it is this thread's.
#[test]
fn js_wait_for_event_reaches_a_timer_deadline_in_at_most_two_turns() {
    let _g = serial();
    std::thread::spawn(|| {
        claim_route();
        let mut clean = false;
        for _attempt in 0..20 {
            crate::timer::js_timer_tick();
            super::super::NOTIFIED.store(false, Ordering::SeqCst);
            let before = stats();
            let start = Instant::now();
            let _promise = crate::timer::js_set_timeout(2.0);
            let mut calls = 0u32;
            let mut fired = 0;
            while fired == 0 {
                calls += 1;
                assert!(calls < 10_000, "js_wait_for_event spun: {calls} calls");
                // `NOTIFIED` from an unrelated thread only shortens a park.
                super::super::NOTIFIED.store(false, Ordering::SeqCst);
                super::super::js_wait_for_event();
                fired = crate::timer::js_timer_tick();
            }
            let after = stats();
            let turns = after.turns - before.turns;
            if calls > 3 {
                // Something woke this park early (GC idle work, a notify);
                // retry for a clean window rather than counting it as a spin.
                continue;
            }
            assert!(start.elapsed() >= Duration::from_millis(2));
            assert!(turns >= 1, "the precise park never turned the loop");
            assert!(turns <= 2, "{turns} turns for one 2 ms deadline");
            let zero = after.zero_event_waits - before.zero_event_waits;
            assert!(zero <= 1, "{zero} zero-event waits for one 2 ms deadline");
            clean = true;
            break;
        }
        assert!(clean, "never observed an uninterrupted timer window");
        shutdown_current_thread();
    })
    .join()
    .unwrap();
}

/// The A/B discriminator itself: while tokio owns in-flight native work the
/// primary agent's park is a TOKIO TICK, not a turnloop turn — and
/// `PERRY_LOOP_STATS` separates the two. Without that separation a server run,
/// which keeps a tokio task alive for as long as it serves, would report
/// turnloop turns it never made.
#[test]
fn native_work_in_flight_is_counted_as_a_tokio_tick_not_a_turn() {
    use super::super::loop_stats;
    let _g = serial();
    loop_stats::force_enable_for_test();

    extern "C" fn always_inflight() -> i32 {
        1
    }
    static TICKS: AtomicU64 = AtomicU64::new(0);
    extern "C" fn counting_tick(_budget_ms: u64) {
        TICKS.fetch_add(1, Ordering::SeqCst);
    }

    claim_route();
    super::super::NOTIFIED.store(false, Ordering::SeqCst);
    super::super::js_register_wait_driver(Some(counting_tick), None, None);
    super::super::js_register_native_inflight(Some(always_inflight));
    let before = loop_stats::snapshot();
    let ticks_before = TICKS.load(Ordering::SeqCst);
    let turns_before = stats().turns;

    // At most five parks: the idle-reclaim hook may consume one by doing GC
    // work (it answers `Resume`, and no wait runs at all). Each attempt is a
    // non-blocking tick, so the loop is bounded and cheap.
    for _ in 0..5 {
        super::super::js_wait_for_event();
        if TICKS.load(Ordering::SeqCst) > ticks_before {
            break;
        }
    }

    let ticks_ran = TICKS.load(Ordering::SeqCst) - ticks_before;
    let turns_ran = stats().turns - turns_before;
    let after = loop_stats::snapshot();
    super::super::js_register_native_inflight(None);
    super::super::js_register_wait_driver(None, None, None);
    shutdown_current_thread();

    assert_eq!(
        ticks_ran, 1,
        "the registered tick is the subject and it never ran"
    );
    assert_eq!(
        after.tokio_tick.count - before.tokio_tick.count,
        ticks_ran,
        "the tick was not counted as a tokio tick"
    );
    assert_eq!(
        after.turnloop.count - before.turnloop.count,
        0,
        "a tokio tick was miscounted as a turnloop turn"
    );
    assert_eq!(turns_ran, 0, "the loop turned while tokio owned the wait");
    super::super::NOTIFIED.store(false, Ordering::SeqCst);
}
