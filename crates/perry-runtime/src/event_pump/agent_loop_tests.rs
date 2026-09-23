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

/// Give this test thread a loop WITHOUT a route slot, so a test that only
/// exercises the turn cannot race another thread for its agent's route.
fn install_unrouted() {
    let agent = AgentLoop::new(
        Profile::Wait,
        crate::agent::current_agent(),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("create agent loop");
    AGENT_LOOP.with(|slot| *slot.borrow_mut() = Some(agent));
    STATE.with(|s| s.set(LoopState::Owner));
}

fn stats() -> LoopStats {
    loop_statistics().expect("this thread owns a loop")
}

/// The subject of the fatal `Loop::new` path: on a host with a real turnloop
/// backend, EVERY profile this runtime asks for is actually constructible.
///
/// `loop_creation_failed` aborts the process, so "nothing threw" is not a
/// verdict here — the point is that a healthy process never reaches it, which
/// requires both halves: a backend exists, and both `Config`s are accepted.
/// `net_config()`'s ceilings have moved twice (perry#10351 handles, the 19 MB
/// `WorkPort` ring), and `Driver::new` rejects a `Config` whose
/// `events_per_turn * 3 + max_operations + max_handles` overflows.
#[test]
fn every_profile_is_constructible_on_a_supported_host() {
    assert_ne!(
        turnloop::BACKEND_NAME,
        "unsupported",
        "a host with no turnloop backend must not reach the fatal Loop::new path"
    );
    for profile in [Profile::Wait, Profile::Net] {
        let agent = AgentLoop::new(
            profile,
            crate::agent::current_agent(),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap_or_else(|error| panic!("{profile:?} profile is not constructible: {error}"));
        assert_eq!(agent.profile, profile);
        drop(agent);
    }
}

/// Claim the primary agent's route on this thread, waiting out a route held by
/// a test thread that is still finishing.
fn take_primary_route() {
    let limit = Instant::now() + Duration::from_secs(10);
    loop {
        STATE.with(|s| s.set(LoopState::Unset));
        release_route();
        if ensure_loop() {
            return;
        }
        assert!(Instant::now() < limit, "primary route never became free");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Whether any thread currently speaks for `agent`.
fn route_taken(agent: crate::agent::AgentId) -> bool {
    ROUTES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .any(|route| route.agent == agent)
}

fn route_is_free() -> bool {
    !route_taken(crate::agent::PRIMARY_AGENT)
}

/// The identity of the loop behind `agent`'s route; 0 when the slot is merely
/// claimed. Two agents whose loops are distinct have distinct ids here.
fn route_loop_id(agent: crate::agent::AgentId) -> u64 {
    ROUTES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .find(|route| route.agent == agent)
        .map_or(0, |route| route.loop_id)
}

/// Spin until `agent`'s owner is blocked inside the OS wait itself, so a wake
/// has to take the syscall path rather than a pre-park notification bit.
fn await_parked(agent: crate::agent::AgentId) {
    let limit = Instant::now() + Duration::from_secs(10);
    loop {
        let parked = {
            let routes = ROUTES.lock().unwrap_or_else(PoisonError::into_inner);
            routes.iter().any(|route| {
                route.agent == agent
                    && route.in_turn.load(Ordering::SeqCst)
                    && route
                        .notifier
                        .as_ref()
                        .is_some_and(|notifier| notifier.is_parked())
            })
        };
        if parked {
            return;
        }
        assert!(Instant::now() < limit, "owner never parked in its turn");
        std::thread::yield_now();
    }
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
    // The spawned owner PARKS (clearing the wake-latency slot) while this
    // thread notifies (setting it), so both must see one instance. Adopted as
    // the owner's first statement -- after its first park it would be too late
    // (`PerThread::adopt`).
    let wake_key = super::super::loop_stats::test_shared_wake_key();
    let owner = std::thread::spawn(move || {
        super::super::loop_stats::test_adopt_wake(wake_key);
        take_primary_route();
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
    await_parked(crate::agent::PRIMARY_AGENT);
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

/// `js_native_work_submitted` is a wake producer of its own — it is how a
/// cross-thread native submission reaches a parked turn, and it does NOT go
/// through `js_notify_main_thread`. It must therefore produce a wake-latency
/// sample too, or the turnloop arm's histogram silently omits exactly the wakes
/// the A/B is about.
#[test]
fn a_cross_thread_native_submission_wakes_a_turn_and_is_one_wake_sample() {
    let _g = serial();
    super::super::loop_stats::force_enable_for_test();
    let before = super::super::loop_stats::snapshot();
    let (parked_tx, parked_rx) = mpsc::channel();
    // The spawned owner PARKS (clearing the wake-latency slot) while this
    // thread notifies (setting it), so both must see one instance. Adopted as
    // the owner's first statement -- after its first park it would be too late
    // (`PerThread::adopt`).
    let wake_key = super::super::loop_stats::test_shared_wake_key();
    let owner = std::thread::spawn(move || {
        super::super::loop_stats::test_adopt_wake(wake_key);
        take_primary_route();
        super::super::NOTIFIED.store(false, Ordering::SeqCst);
        parked_tx.send(()).unwrap();
        let start = Instant::now();
        let park = park_until(start + Duration::from_secs(30));
        let waited = start.elapsed();
        shutdown_current_thread();
        (park, waited)
    });
    parked_rx.recv().unwrap();
    await_parked(crate::agent::PRIMARY_AGENT);
    super::super::js_native_work_submitted();
    let (park, waited) = owner.join().unwrap();
    assert_eq!(park, Park::Waited);
    assert!(
        waited < Duration::from_secs(10),
        "the native-submission wake was lost: waited {waited:?}"
    );
    let after = super::super::loop_stats::snapshot();
    assert_eq!(
        after.turnloop.count - before.turnloop.count,
        1,
        "the turn is the subject and it did not run"
    );
    assert_eq!(
        after.wake_samples() - before.wake_samples(),
        1,
        "a cross-thread native submission must produce one wake-latency sample"
    );
    super::super::NOTIFIED.store(false, Ordering::SeqCst);
}

/// Install on first use, idempotent shutdown, no reinstall afterwards, and the
/// route is released both by shutdown and by plain thread exit.
#[test]
fn install_shutdown_and_thread_exit_release_the_loop_and_route() {
    let _g = serial();
    std::thread::spawn(|| {
        take_primary_route();
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
    std::thread::spawn(take_primary_route).join().unwrap();
    assert!(route_is_free(), "thread exit kept the route");
}

/// turnloop P9: a worker agent gets a loop of its own, and the two predicates
/// that decide whether a submission is accepted agree with each other.
///
/// `net_available()` and `ensure_loop_with()` disagreeing is the class
/// `c13372cc70` fixed from the other side — a `worker_threads` Worker's
/// `fetch()` was accepted by the submit guard and refused a moment later, so
/// it failed after acceptance instead of falling back. Asserting them together
/// on the same thread is what keeps that closed.
#[test]
fn a_worker_agent_gets_its_own_loop() {
    std::thread::spawn(|| {
        let agent = crate::agent::enter_worker_agent();
        assert_ne!(agent, crate::agent::PRIMARY_AGENT);
        assert!(
            net_available(),
            "a worker agent must be able to take the turnloop net path"
        );
        assert!(eligible(), "a worker agent must be able to park precisely");
        assert!(
            ensure_loop(),
            "net_available() promised a loop it cannot get"
        );
        assert!(loop_statistics().is_some());
        assert!(route_taken(agent), "the worker claimed no route");
        // The teardown a worker agent actually takes.
        crate::agent::retire_agent(agent);
        assert!(
            !route_taken(agent),
            "retiring the agent left its route installed"
        );
        assert!(loop_statistics().is_none(), "retire kept the loop");
        assert!(!net_available(), "a retired agent must not re-arm");
    })
    .join()
    .unwrap();
}

/// Two worker agents get two independent loops, and neither takes the
/// primary agent's route.
#[test]
fn sibling_worker_agents_do_not_share_a_loop() {
    let _g = serial();
    let before = routed_agents();
    let (a_tx, a_rx) = mpsc::channel();
    let (go_tx, go_rx) = mpsc::channel::<()>();
    let a = std::thread::spawn(move || {
        let agent = crate::agent::enter_worker_agent();
        assert!(ensure_loop());
        let loop_id = route_loop_id(agent);
        assert_ne!(loop_id, 0, "no endpoint published for this agent");
        a_tx.send((agent, loop_id)).unwrap();
        // Hold the loop until the sibling has built its own, so both exist at
        // once — a sequential pair would pass even with one shared route.
        go_rx.recv().unwrap();
        crate::agent::retire_agent(agent);
    });
    let (a_agent, a_loop) = a_rx.recv().unwrap();
    let b = std::thread::spawn(move || {
        let agent = crate::agent::enter_worker_agent();
        assert!(ensure_loop());
        let loop_id = route_loop_id(agent);
        assert_ne!(loop_id, 0, "no endpoint published for this agent");
        crate::agent::retire_agent(agent);
        (agent, loop_id)
    });
    let (b_agent, b_loop) = b.join().unwrap();
    go_tx.send(()).unwrap();
    a.join().unwrap();
    assert_ne!(a_agent, b_agent);
    assert_ne!(a_loop, b_loop, "two agents shared one loop");
    assert_eq!(
        routed_agents(),
        before,
        "retired worker agents leaked route slots"
    );
}

/// A second thread acting for an agent that already has an owner keeps the
/// legacy park. This is the Android shape — `perry-native` runs the JS and
/// owns the loop, the UI thread pumps on its behalf — and it must stay
/// exactly one owner per agent.
///
/// It is also what keeps the two causes of `LoopState::Declined` separated now
/// that one of them aborts: this decline is decided by `claim_route`, so the
/// pump thread must never reach `AgentLoop::new`. `loop_statistics().is_none()`
/// is that assertion — and if the routing regressed, `loop_creation_failed`
/// would take down the whole test binary rather than let it pass.
#[test]
fn a_second_thread_of_the_same_agent_is_declined() {
    let _g = serial();
    let (owned_tx, owned_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let owner = std::thread::spawn(move || {
        let agent = crate::agent::enter_worker_agent();
        assert!(ensure_loop());
        owned_tx.send(agent).unwrap();
        done_rx.recv().unwrap();
        crate::agent::retire_agent(agent);
    });
    let agent = owned_rx.recv().unwrap();
    std::thread::spawn(move || {
        // Same agent id, different thread: a pump, not an owner.
        crate::agent::enter_agent_for_test(agent);
        assert!(!net_available(), "two threads claimed one agent's loop");
        assert!(!eligible());
        assert!(!ensure_loop());
        assert!(loop_statistics().is_none());
    })
    .join()
    .unwrap();
    done_tx.send(()).unwrap();
    owner.join().unwrap();
}

/// `js_notify_main_thread` is a broadcast: a Worker parked in its OWN turn has
/// to be woken by it, or a `postMessage`-driven resolution leaves that agent
/// asleep — a hang, not an error.
#[test]
fn a_notify_wakes_a_parked_worker_agent() {
    let _g = serial();
    let (parked_tx, parked_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let agent = crate::agent::enter_worker_agent();
        assert!(ensure_loop());
        super::super::NOTIFIED.store(false, Ordering::SeqCst);
        parked_tx.send(agent).unwrap();
        let start = Instant::now();
        let park = park_until(start + Duration::from_secs(30));
        let waited = start.elapsed();
        let stats = stats();
        crate::agent::retire_agent(agent);
        (park, waited, stats)
    });
    let agent = parked_rx.recv().unwrap();
    await_parked(agent);
    super::super::js_notify_main_thread();
    let (park, waited, stats) = worker.join().unwrap();
    assert_eq!(park, Park::Waited);
    assert!(
        waited < Duration::from_secs(10),
        "a parked worker agent missed the wake: waited {waited:?}"
    );
    assert_eq!(stats.turns, 1, "{stats:?}");
    assert_eq!(stats.os_waits, 1, "{stats:?}");
    super::super::NOTIFIED.store(false, Ordering::SeqCst);
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
        take_primary_route();
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

    take_primary_route();
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

/// turnloop P3: the armed JS-timer deadline must NOT keep the loop alive.
///
/// A timer operation on a referenced handle counts toward turnloop's `refs`, so
/// without the `set_ref(handle, false)` in `arm_timer` an armed deadline would
/// make `Loop::alive()` true on its own and take the keep-alive decision away
/// from Perry's own counters. This is the sabotage check for that one line: it
/// fails if the `set_ref` is dropped.
#[test]
fn an_armed_timer_deadline_does_not_keep_the_loop_alive() {
    let _g = serial();
    std::thread::spawn(|| {
        install_unrouted();
        assert!(
            !AGENT_LOOP.with(|slot| slot.borrow().as_ref().unwrap().driver.alive()),
            "the loop must start with nothing keeping it alive"
        );

        let at = Instant::now() + Duration::from_secs(3600);
        arm_timer(Some(at));

        let armed = AGENT_LOOP.with(|slot| slot.borrow().as_ref().unwrap().timer);
        assert!(armed.is_some(), "the subject was never armed");
        assert_eq!(armed.map(|(_, at)| at), Some(at));
        assert_eq!(
            loop_deadline(),
            Some(at),
            "the loop's own deadline must carry the JS timer deadline"
        );
        assert!(
            !AGENT_LOOP.with(|slot| slot.borrow().as_ref().unwrap().driver.alive()),
            "an armed JS timer deadline must not answer alive() by itself"
        );
        assert_eq!(stats().timer_arms, 1);

        // Moving an armed deadline reuses the handle rather than churning it.
        let later = at + Duration::from_secs(1);
        arm_timer(Some(later));
        assert_eq!(
            AGENT_LOOP.with(|slot| slot.borrow().as_ref().unwrap().timer.map(|(h, _)| h.key())),
            armed.map(|(h, _)| h.key()),
            "a deadline move must reset the handle, not replace it"
        );
        assert_eq!(loop_deadline(), Some(later));

        // Re-arming at the same instant is a no-op, not another submission.
        let arms = stats().timer_arms;
        arm_timer(Some(later));
        assert_eq!(stats().timer_arms, arms, "an unchanged deadline re-armed");

        arm_timer(None);
        assert_eq!(loop_deadline(), None, "disarming left a deadline behind");
        shutdown_current_thread();
    })
    .join()
    .expect("timer arming test thread");
}

/// turnloop P3: a JS timer's expiry arrives as a real `OpResult::Timer`
/// completion, and the loop parks on it rather than returning early.
#[test]
fn an_armed_timer_expiry_is_a_turnloop_completion() {
    let _g = serial();
    std::thread::spawn(|| {
        install_unrouted();
        let before = stats();
        let at = Instant::now() + Duration::from_millis(5);
        arm_timer(Some(at));

        // One turn with a generous cap: the wait must end at the deadline.
        turn_for_test(Duration::from_millis(500));

        let after = stats();
        assert!(
            Instant::now() >= at,
            "the turn returned before the armed deadline"
        );
        assert_eq!(
            after.timer_expiries - before.timer_expiries,
            1,
            "the armed deadline did not arrive as a turnloop timer completion"
        );
        assert!(
            after.os_waits - before.os_waits >= 1,
            "the park did not reach an OS wait"
        );
        assert_eq!(
            loop_deadline(),
            None,
            "an expired one-shot timer must not leave a deadline armed"
        );
        shutdown_current_thread();
    })
    .join()
    .expect("timer expiry test thread");
}

/// turnloop P3: the deadline the loop is armed at and the deadline Perry
/// computes for its own park are the same instant. They are two reads of one
/// heap root, and a park that used only one of them must be exact — the
/// arming covers the loop-owning thread, Perry's own read covers a worker or
/// an Android pump thread that has no loop.
#[test]
fn the_armed_deadline_and_perrys_own_deadline_agree() {
    let _g = serial();
    std::thread::spawn(|| {
        install_unrouted();
        // #340/#341: this returns the handle OBJECT's raw pointer, not the raw
        // registry id -- codegen NaN-boxes the i64 return. So the clear below
        // has to box it and go through the value entry point, which is also
        // the one codegen emits for `clearTimeout`
        // (`lower_call/namespace_call.rs` -> the `_value` form).
        let handle = crate::timer::js_set_timeout_callback(0, 50_000.0);
        assert!(handle != 0, "the subject timer was never scheduled");
        let handle_value = crate::value::js_nanbox_pointer(handle);
        let heap = crate::timer::next_timer_deadline();
        assert!(heap.is_some(), "the timer heap has no deadline to compare");
        assert_eq!(
            loop_deadline(),
            heap,
            "the armed deadline drifted from the heap root"
        );
        crate::timer::js_clear_timeout_value(handle_value);
        assert_eq!(crate::timer::next_timer_deadline(), None);
        assert_eq!(
            loop_deadline(),
            None,
            "a cleared timer left a deadline armed"
        );
        shutdown_current_thread();
    })
    .join()
    .expect("deadline agreement test thread");
}

/// One turn of this thread's own loop, staged but deliberately NOT dispatched,
/// so a test can read the completions the turn actually produced. The real
/// `settle_turn` dispatches, which would hand a synthetic token to a router.
fn turn_and_stage(budget: Duration) {
    AGENT_LOOP.with(|slot| {
        let mut slot = slot.borrow_mut();
        let agent = slot.as_mut().expect("this thread owns a loop");
        let timeout = if budget.is_zero() {
            Timeout::Now
        } else {
            Timeout::After(budget)
        };
        if let Ok(info) = agent.driver.turn(timeout, &mut agent.completions) {
            agent.record(&info);
        }
    });
}

/// perry#10395: a thread that does **not** own an agent's loop can still hand
/// that loop work.
///
/// This is the enabler a host pump thread needs. Since P9 every JS agent owns
/// its own loop, but a second thread legitimately acts *for* an agent another
/// thread owns — a host pump, Android's UI thread — and until now its only
/// answer was to decline to tokio, which is why the tokio implementations are
/// still live code behind those bindings.
///
/// Asserts the mechanism, not the absence of a panic: the payload arrives on
/// the **owner**, exactly once, with its token and value intact, and an agent
/// nobody speaks for gets a named error instead of a silent decline.
#[test]
fn a_foreign_thread_posts_work_to_an_agents_owner() {
    const POSTED: Token = Token(0x1_0395);
    const VALUE: u64 = 0x00C0_FFEE;

    let _g = serial();
    take_primary_route();
    let agent = crate::agent::current_agent();

    // "No loop at all" and "that loop refused this post" call for different
    // answers from the caller, so they must not collapse into one failure.
    let unrouted = u64::MAX;
    assert!(!route_taken(unrouted), "no agent id is this one");
    assert!(
        matches!(
            post_to_agent(unrouted, POSTED, Payload::U64(VALUE)),
            Err(PostToAgentError::NoRoute)
        ),
        "a post to an agent with no route must name NoRoute"
    );

    // Clear whatever an earlier test staged, so the count below is this post's.
    turn_and_stage(Duration::ZERO);
    STAGED.with(|staged| staged.borrow_mut().clear());
    let before = stats().completions;

    // A second thread, acting FOR this agent, hands the owner work.
    let (tx, rx) = mpsc::channel();
    let foreign = std::thread::spawn(move || {
        tx.send(post_to_agent(agent, POSTED, Payload::U64(VALUE)).is_ok())
            .ok();
    });
    assert!(
        rx.recv_timeout(Duration::from_secs(10))
            .expect("the foreign thread reported"),
        "the foreign post was not accepted"
    );
    foreign.join().expect("foreign poster thread");

    // The owner picks it up in its own turn. `post` woke us, so this returns
    // well inside the budget rather than at it.
    let limit = Instant::now() + Duration::from_secs(10);
    while STAGED.with(|staged| staged.borrow().is_empty()) {
        assert!(
            Instant::now() < limit,
            "the posted payload never reached the owner"
        );
        turn_and_stage(Duration::from_millis(50));
    }

    STAGED.with(|staged| {
        let staged = staged.borrow();
        assert_eq!(
            staged.len(),
            1,
            "exactly one completion — a post is delivered once, not retried"
        );
        let completion = &staged[0];
        assert_eq!(completion.token, POSTED, "the routing token survived");
        assert!(
            completion.handle.is_none(),
            "an unsolicited post owns no handle"
        );
        match &completion.result {
            turnloop::OpResult::Posted(Payload::U64(value)) => {
                assert_eq!(*value, VALUE, "the payload survived the thread hop");
            }
            other => panic!("expected the posted payload, got {other:?}"),
        }
    });
    assert_eq!(
        stats().completions,
        before + 1,
        "the owner counted the post exactly once"
    );

    // A synthetic token must never reach the routers.
    STAGED.with(|staged| staged.borrow_mut().clear());
}

/// A refused post hands the payload **back**, so the caller can retry instead
/// of losing the work.
///
/// This is the half of `Poster::post`'s contract that is easy to get wrong:
/// `payload: None` does NOT mean "nothing to retry", it means the post was
/// *accepted* and a later wake failed — retrying there would deliver twice.
/// Only `Some` licenses a retry, so `PostToAgentError::Refused` has to carry
/// the distinction through rather than flatten it into one error.
#[test]
fn a_full_postbox_hands_the_payload_back_so_the_caller_can_retry() {
    const POSTED: Token = Token(0x1_0396);

    let _g = serial();
    take_primary_route();
    let agent = crate::agent::current_agent();

    turn_and_stage(Duration::ZERO);
    STAGED.with(|staged| staged.borrow_mut().clear());

    // Fill the postbox without turning, so nothing drains behind us.
    let mut accepted = 0u64;
    let refused = loop {
        match post_to_agent(agent, POSTED, Payload::U64(accepted)) {
            Ok(()) => {
                accepted += 1;
                assert!(accepted < 1_000_000, "the postbox is not bounded at all");
            }
            Err(err) => break err,
        }
    };
    assert!(accepted > 0, "the postbox accepted nothing at all");
    match refused {
        PostToAgentError::Refused {
            payload: Some(Payload::U64(value)),
        } => assert_eq!(value, accepted, "the refused payload came back intact"),
        other => panic!("a full postbox must hand the payload back, got {other:?}"),
    }

    // Drain it, so the next test starts on an empty loop — and so that the
    // count proves every accepted post arrives, not merely the first.
    let limit = Instant::now() + Duration::from_secs(10);
    let mut delivered = 0u64;
    while delivered < accepted {
        assert!(Instant::now() < limit, "the accepted posts never drained");
        turn_and_stage(Duration::from_millis(50));
        delivered += STAGED.with(|staged| {
            let mut staged = staged.borrow_mut();
            let mine = staged.iter().filter(|c| c.token == POSTED).count() as u64;
            staged.clear();
            mine
        });
    }
    assert_eq!(
        delivered, accepted,
        "every accepted post was delivered exactly once"
    );
}

/// perry#10395 step 2: the posted **host job** — the shape a binding crate can
/// reach across the C ABI, where `post_to_agent`'s `Payload` cannot go.
///
/// This is the end of the decline. A thread acting for an agent another thread
/// owns used to have exactly one answer — keep a tokio driver — and now has
/// this: hand the work to the owner, which is a thread serving the *same JS
/// heap*, so the result is built where that agent's values live.
///
/// Asserts the mechanism rather than the absence of a panic: the callback runs
/// **on the owner and not on the poster**, exactly once, with its context
/// intact, and the runtime's liveness counter moves so a green run cannot mean
/// "nothing listened".
#[test]
fn a_posted_host_job_runs_on_the_owner_not_on_the_poster() {
    use crate::turnloop_post::{self, Posted};
    use std::os::raw::c_void;
    use std::sync::atomic::AtomicUsize;

    struct Job {
        value: u64,
        ran_on: Option<std::thread::ThreadId>,
        dropped: &'static AtomicUsize,
    }
    impl Drop for Job {
        fn drop(&mut self) {
            self.dropped.fetch_add(1, Ordering::SeqCst);
        }
    }
    per_test_global! {
        /// DONE / SEEN_VALUE / SEEN_THREAD are written by `run`, which executes
        /// on the OWNER — this test's own thread — so they need no adoption.
        /// DROPPED does: the POSTER thread captures `&DROPPED` into the `Job`,
        /// and the per-thread split would hand it the poster's instance while
        /// the assertions below read this thread's. The poster adopts this
        /// thread's key as its first statement (see the spawn below).
        static DROPPED: AtomicUsize = AtomicUsize::new(0);
        static DONE: AtomicUsize = AtomicUsize::new(0);
        static SEEN_VALUE: AtomicU64 = AtomicU64::new(0);
        static SEEN_THREAD: Mutex<Option<std::thread::ThreadId>> = Mutex::new(None);
    }

    extern "C" fn run(ctx: *mut c_void) {
        // SAFETY: the runtime hands back exactly the context `post` was given,
        // exactly once, and this is that invocation.
        let mut job = unsafe { Box::from_raw(ctx.cast::<Job>()) };
        job.ran_on = Some(std::thread::current().id());
        SEEN_VALUE.store(job.value, Ordering::SeqCst);
        *SEEN_THREAD.lock().unwrap_or_else(PoisonError::into_inner) = job.ran_on;
        DONE.fetch_add(1, Ordering::SeqCst);
    }

    const VALUE: u64 = 0x0DEF_ACED;

    let _g = serial();
    take_primary_route();
    let owner = std::thread::current().id();
    assert!(
        turnloop_post::available(),
        "this thread owns the primary agent's published loop, so a post lands"
    );

    // Must be taken on THIS thread, and adopted by the poster before its first
    // touch of the table — adopting later silently orphans what it already
    // wrote (`PerThread::adopt`).
    let dropped_key = DROPPED.shared_key();
    let poster = std::thread::spawn(move || {
        DROPPED.adopt(dropped_key);
        // A second thread acting FOR the primary agent: it has no agent of its
        // own, so `current_agent()` resolves to PRIMARY_AGENT — the Android
        // UI-thread shape, and the reason the tokio drivers are still alive.
        assert_ne!(
            std::thread::current().id(),
            owner,
            "the poster must not be the owner, or this proves nothing"
        );
        assert!(turnloop_post::available(), "the owner's route is published");
        let ctx = Box::into_raw(Box::new(Job {
            value: VALUE,
            ran_on: None,
            dropped: &DROPPED,
        }));
        // SAFETY: a live leaked box; on a non-negative outcome the runtime owns
        // it and `run` consumes it, on a negative one this thread reclaims it.
        let outcome = unsafe { turnloop_post::post(run, ctx.cast()) };
        if !outcome.consumed() {
            // SAFETY: refused, so the box is still ours.
            drop(unsafe { Box::from_raw(ctx) });
        }
        outcome
    });
    let outcome = poster.join().expect("posting thread");
    assert_eq!(
        outcome,
        Posted::Accepted,
        "a published loop with an empty postbox accepts and wakes"
    );

    let before = turnloop_post::dispatched();
    let limit = Instant::now() + Duration::from_secs(10);
    while turnloop_post::dispatched() == before {
        assert!(
            Instant::now() < limit,
            "the posted job never reached the owner"
        );
        settle_turn();
    }

    assert_eq!(
        turnloop_post::dispatched(),
        before + 1,
        "the owner ran the job exactly once — a post is delivered, not retried"
    );
    assert_eq!(DONE.load(Ordering::SeqCst), 1, "the callback ran once");
    assert_eq!(
        SEEN_VALUE.load(Ordering::SeqCst),
        VALUE,
        "the context survived the thread hop intact"
    );
    assert_eq!(
        *SEEN_THREAD.lock().unwrap_or_else(PoisonError::into_inner),
        Some(owner),
        "the job ran on the agent's OWNER, which is the whole point: that is \
         the thread where this agent's JS values live"
    );
    assert_eq!(
        DROPPED.load(Ordering::SeqCst),
        1,
        "the context was consumed exactly once, by the callback"
    );
}
