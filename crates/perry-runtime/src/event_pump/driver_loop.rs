//! P0 wait adapter. JS callbacks and timer ownership remain in Perry until P3.
//! P8 deletes the native Tokio coexistence branch and the measurement feature.
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use turnloop::{Completions, Config, Loop, Notifier, Timeout};

struct Wake {
    notifier: Notifier,
    pending: AtomicBool,
}

impl Wake {
    fn notify(&self) {
        self.pending.store(true, Ordering::Release);
        // A racing shutdown closes the notifier; there is then no waiter.
        let _ = self.notifier.notify();
    }
}

// P0's legacy js_notify_main_thread route addresses only the primary agent.
// This is a notifier, never a process-global Loop. P3/P4 add addressed producers.
static PRIMARY_WAKE: Mutex<Option<Arc<Wake>>> = Mutex::new(None);
thread_local! {
    static IS_OWNER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static AGENT_LOOP: RefCell<Option<AgentLoop>> = const { RefCell::new(None) };
}

#[derive(Default, Debug)]
struct Stats {
    turns: u64,
    os_waits: u64,
    zero_event_waits: u64,
    native_ticks: u64,
}

struct AgentLoop {
    driver: Loop,
    completions: Completions,
    wake: Arc<Wake>,
    stats: Stats,
}

impl AgentLoop {
    fn new() -> Self {
        let driver = Loop::new(Config::default()).expect("create Perry agent loop");
        let wake = Arc::new(Wake {
            notifier: driver.notifier(),
            pending: AtomicBool::new(false),
        });
        Self {
            driver,
            completions: Completions::default(),
            wake,
            stats: Stats::default(),
        }
    }

    fn turn(&mut self, timeout: Timeout) {
        self.wake.pending.store(false, Ordering::Release);
        let info = self
            .driver
            .turn(timeout, &mut self.completions)
            .expect("Perry agent loop wait");
        self.stats.turns += 1;
        self.stats.os_waits += u64::from(info.os_waits);
        self.stats.zero_event_waits += u64::from(info.zero_event_waits);
        // P0 submits no operations. P1 must dispatch completions here, after
        // turn returns, before releasing any JS roots associated with tokens.
        debug_assert!(self.completions.is_empty());
    }
}

impl Drop for AgentLoop {
    fn drop(&mut self) {
        // Rust-thread exit is also a teardown path (unit tests and embedders).
        // Remove only this agent's route, including when shutdown was implicit.
        let mut primary = PRIMARY_WAKE.lock().unwrap();
        if primary
            .as_ref()
            .is_some_and(|wake| Arc::ptr_eq(wake, &self.wake))
        {
            primary.take();
        }
        drop(primary);
        if std::env::var("PERRY_LOOP_STATS").as_deref() == Ok("1") {
            eprintln!("[perry-loop] driver=turnloop turns={} os_waits={} zero_event_waits={} native_ticks={}",
                self.stats.turns, self.stats.os_waits, self.stats.zero_event_waits, self.stats.native_ticks);
        }
        // Loop::drop closes its notifier and native backend; no pool jobs or
        // handles are owned here in P0, so destruction has no blocking join.
    }
}

pub fn install_turnloop_driver() {
    if crate::agent::current_agent() != crate::agent::PRIMARY_AGENT {
        return;
    }
    AGENT_LOOP.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_some() {
            return;
        }
        let mut primary = PRIMARY_WAKE.lock().unwrap();
        if primary.is_some() {
            return;
        }
        let agent = AgentLoop::new();
        *primary = Some(agent.wake.clone());
        *slot = Some(agent);
        IS_OWNER.with(|owner| owner.set(true));
        crate::event_pump::register_precise_wait_driver(sleep, fast, shutdown);
        if super::WAIT_DRIVER_SLEEP.load(Ordering::Acquire).is_null() {
            crate::event_pump::js_register_wait_driver(None, None, Some(wake));
        }
    });
}

#[derive(Clone, Copy)]
struct NativeBridge {
    inflight: fn() -> bool,
    sleep: fn(Instant),
    fast: fn(),
}
thread_local! {
    static NATIVE: std::cell::Cell<Option<NativeBridge>> = const { std::cell::Cell::new(None) };
}

/// P0-transitional callbacks supplied by stdlib; runtime has no Tokio dependency.
pub fn register_native_wait_bridge(inflight: fn() -> bool, sleep: fn(Instant), fast: fn()) {
    NATIVE.with(|slot| {
        slot.set(Some(NativeBridge {
            inflight,
            sleep,
            fast,
        }))
    });
}

pub extern "C" fn wake() {
    // A producer on the JS thread cannot race its own park. The runtime's
    // NOTIFIED bit already schedules its next pump; notifying turnloop here
    // would leave a stale notification that forces a zero-event OS poll.
    if IS_OWNER.with(std::cell::Cell::get) {
        return;
    }
    if let Some(wake) = PRIMARY_WAKE.lock().unwrap().as_ref() {
        wake.notify();
    }
}

fn shutdown() {
    IS_OWNER.with(|owner| owner.set(false));
    PRIMARY_WAKE.lock().unwrap().take();
    NATIVE.with(|slot| slot.set(None));
    AGENT_LOOP.with(|slot| slot.borrow_mut().take());
}

fn native_bridge() -> Option<NativeBridge> {
    NATIVE
        .with(|slot| slot.get())
        .filter(|bridge| (bridge.inflight)())
}

fn sleep(deadline: Instant) {
    if let Some(bridge) = native_bridge() {
        AGENT_LOOP.with(|slot| slot.borrow_mut().as_mut().unwrap().stats.native_ticks += 1);
        (bridge.sleep)(deadline);
        return;
    }
    AGENT_LOOP.with(|slot| {
        slot.borrow_mut()
            .as_mut()
            .unwrap()
            .turn(Timeout::Until(deadline))
    });
}

fn fast() {
    if let Some(bridge) = native_bridge() {
        AGENT_LOOP.with(|slot| slot.borrow_mut().as_mut().unwrap().stats.native_ticks += 1);
        (bridge.fast)();
    }
    AGENT_LOOP.with(|slot| {
        let mut slot = slot.borrow_mut();
        let agent = slot.as_mut().unwrap();
        if agent.driver.alive() || agent.wake.pending.load(Ordering::Acquire) {
            agent.turn(Timeout::Now);
        }
    });
}

pub(super) fn next_deadline() -> Option<Instant> {
    AGENT_LOOP.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|agent| agent.driver.next_deadline())
    })
}

/// Diagnostic snapshot: turns, OS waits, zero-event waits, transitional ticks.
pub fn loop_statistics() -> Option<[u64; 4]> {
    AGENT_LOOP.with(|slot| {
        slot.borrow().as_ref().map(|agent| {
            [
                agent.stats.turns,
                agent.stats.os_waits,
                agent.stats.zero_event_waits,
                agent.stats.native_ticks,
            ]
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn quiet_fractional_deadlines_with_idle_socket_do_not_spin() {
        for micros in [500, 2000, 10000] {
            let mut agent = AgentLoop::new();
            let listener = agent
                .driver
                .tcp_listen(
                    "127.0.0.1:0".parse().unwrap(),
                    &turnloop::ListenOpts::default(),
                )
                .unwrap();
            let deadline = Instant::now() + Duration::from_micros(micros);
            while Instant::now() < deadline {
                assert!(agent.stats.turns < 2, "{micros} us spun: {:?}", agent.stats);
                agent.turn(Timeout::Until(deadline));
            }
            assert!(agent.stats.turns > 0, "deadline wait never ran");
            assert_eq!(agent.stats.os_waits, 1, "{:?}", agent.stats);
            assert!(agent.stats.zero_event_waits <= 1, "{:?}", agent.stats);
            agent.driver.close(listener, turnloop::Token(1)).unwrap();
            agent
                .driver
                .turn(Timeout::Now, &mut agent.completions)
                .unwrap();
            assert!(!agent.driver.alive());
        }
    }

    #[test]
    fn another_thread_wakes_a_parked_loop_and_drop_closes_notifier() {
        let mut agent = AgentLoop::new();
        let wake = agent.wake.clone();
        let thread_wake = wake.clone();
        let producer = std::thread::spawn(move || {
            let limit = Instant::now() + Duration::from_secs(5);
            while !thread_wake.notifier.is_parked() {
                assert!(Instant::now() < limit, "subject never parked");
                std::thread::yield_now();
            }
            thread_wake.notify();
        });
        agent.turn(Timeout::Until(Instant::now() + Duration::from_secs(5)));
        producer.join().unwrap();
        assert_eq!(agent.stats.turns, 1);
        assert_eq!(agent.stats.os_waits, 1);
        assert!(
            wake.notifier.wake_syscalls() > 0,
            "cross-thread OS wake never ran"
        );
        drop(agent);
        assert!(
            wake.notifier.notify().is_err(),
            "loop drop left notifier open"
        );
    }

    #[test]
    fn install_fast_idle_and_shutdown() {
        install_turnloop_driver();
        AGENT_LOOP.with(|slot| assert!(slot.borrow().is_some()));
        fast();
        AGENT_LOOP.with(|slot| assert_eq!(slot.borrow().as_ref().unwrap().stats.turns, 0));
        crate::event_pump::shutdown_wait_driver();
        AGENT_LOOP.with(|slot| assert!(slot.borrow().is_none()));
        assert!(PRIMARY_WAKE.lock().unwrap().is_none());
    }
}
