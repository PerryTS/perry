//! Agent-local precise driver. Rust function pointers keep `Instant` out of the
//! C ABI; the legacy C millisecond registration remains the A/B and worker path.
use std::cell::Cell;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
struct Driver {
    sleep: fn(Instant),
    fast: fn(),
    shutdown: fn(),
}

thread_local! {
    static DRIVER: Cell<Option<Driver>> = const { Cell::new(None) };
    static DEADLINE_PROVIDER: Cell<Option<fn() -> Option<Instant>>> = const { Cell::new(None) };
}

/// Install on the calling JS agent only. Shutdown clears the slot before
/// releasing its resources, so teardown is idempotent and cannot reenter it.
pub fn register_precise_wait_driver(sleep: fn(Instant), fast: fn(), shutdown: fn()) {
    DRIVER.with(|slot| {
        slot.set(Some(Driver {
            sleep,
            fast,
            shutdown,
        }))
    });
}

/// Register the stdlib deadline without converting its monotonic clock to C data.
pub fn register_stdlib_deadline_provider(provider: fn() -> Option<Instant>) {
    DEADLINE_PROVIDER.with(|slot| slot.set(Some(provider)));
}

pub fn shutdown_wait_driver() {
    DEADLINE_PROVIDER.with(|slot| slot.set(None));
    if let Some(driver) = DRIVER.with(|slot| slot.take()) {
        (driver.shutdown)();
    }
}

pub(super) fn installed() -> bool {
    DRIVER.with(|slot| slot.get().is_some())
}

pub(super) fn sleep(deadline: Instant) -> bool {
    if let Some(driver) = DRIVER.with(|slot| slot.get()) {
        (driver.sleep)(deadline);
        true
    } else {
        false
    }
}

pub(super) fn fast() -> bool {
    if let Some(driver) = DRIVER.with(|slot| slot.get()) {
        (driver.fast)();
        true
    } else {
        false
    }
}

pub(super) fn next_deadline(now: Instant) -> Instant {
    let mut deadline = now + Duration::from_millis(super::IDLE_CAP_MS);
    for at in [
        crate::timer::js_timer_deadline(),
        crate::timer::js_callback_timer_deadline(),
        crate::timer::js_interval_timer_deadline(),
        super::driver_loop::next_deadline(),
    ]
    .into_iter()
    .flatten()
    {
        deadline = deadline.min(at);
    }
    if let Some(provider) = DEADLINE_PROVIDER.with(Cell::get) {
        if let Some(at) = provider() {
            deadline = deadline.min(at);
        }
    } else {
        // Compatibility for C embedders. Sample after the provider returns so
        // conversion can never manufacture an early, zero-budget retry.
        let ms = crate::stdlib_pump::stdlib_next_wake_ms();
        if ms.is_finite() && ms >= 0.0 {
            if let Some(at) = Instant::now().checked_add(Duration::from_secs_f64(ms / 1000.0)) {
                deadline = deadline.min(at);
            }
        }
    }
    deadline
}

#[cfg(test)]
mod tests {
    use super::*;
    thread_local! {
        static CALLS: Cell<usize> = const { Cell::new(0) };
        static DEADLINE: Cell<Option<Instant>> = const { Cell::new(None) };
    }

    #[test]
    fn install_exact_deadline_and_uninstall_are_agent_local() {
        shutdown_wait_driver();
        assert!(!installed());
        register_precise_wait_driver(
            |deadline| DEADLINE.with(|slot| slot.set(Some(deadline))),
            || CALLS.with(|slot| slot.set(slot.get() + 1)),
            || CALLS.with(|slot| slot.set(slot.get() + 10)),
        );
        let deadline = Instant::now() + Duration::from_micros(500);
        assert!(sleep(deadline));
        assert_eq!(DEADLINE.with(Cell::get), Some(deadline));
        assert!(fast());
        std::thread::spawn(|| assert!(!installed())).join().unwrap();
        shutdown_wait_driver();
        shutdown_wait_driver();
        assert!(!sleep(deadline));
        assert_eq!(CALLS.with(Cell::get), 11);
    }
}
