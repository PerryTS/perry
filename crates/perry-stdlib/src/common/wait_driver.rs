//! P0-transitional stdlib registration. P8 deletes the Tokio callbacks.
#[cfg(all(feature = "async-runtime", not(feature = "tokio-wait-driver")))]
use std::time::Instant;

pub(super) fn install() {
    #[cfg(feature = "async-runtime")]
    super::async_bridge::install_legacy_wait_driver();
    #[cfg(not(feature = "tokio-wait-driver"))]
    {
        perry_runtime::event_pump::install_turnloop_driver();
        perry_runtime::event_pump::register_stdlib_deadline_provider(
            crate::readline::next_deadline,
        );
        #[cfg(feature = "async-runtime")]
        perry_runtime::event_pump::register_native_wait_bridge(
            super::async_bridge::native_inflight,
            native_sleep,
            super::async_bridge::run_native_fast_tick,
        );
    }
}

#[cfg(all(feature = "async-runtime", not(feature = "tokio-wait-driver")))]
fn native_sleep(deadline: Instant) {
    // P0 coexistence: exactly the previous tick while Tokio work is in flight.
    // Quiet waits use Instant directly in the runtime's turnloop adapter.
    super::async_bridge::run_one_tick(
        deadline
            .saturating_duration_since(Instant::now())
            .as_millis() as u64,
    );
}

pub(super) extern "C" fn wake() {
    perry_runtime::event_pump::wake_turnloop_driver();
}

pub(super) extern "C" fn next_wake_ms() -> f64 {
    // Loop-owned deadlines are combined as Instants by event_pump::precise.
    // Keep readline's independent ESC deadline until its P3 migration.
    crate::readline::js_readline_next_wake_ms()
}

#[cfg(all(test, feature = "async-runtime", not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn stdlib_installs_selected_driver_and_drives_a_real_native_task() {
        perry_runtime::event_pump::shutdown_wait_driver();
        install();
        #[cfg(not(feature = "tokio-wait-driver"))]
        assert!(perry_runtime::event_pump::loop_statistics().is_some());
        #[cfg(feature = "tokio-wait-driver")]
        assert!(perry_runtime::event_pump::loop_statistics().is_none());
        static RAN: AtomicBool = AtomicBool::new(false);
        super::super::async_bridge::spawn(async {
            RAN.store(true, Ordering::Release);
        });
        assert!(!RAN.load(Ordering::Acquire), "task must start undriven");
        perry_runtime::event_pump::js_wait_for_event();
        assert!(RAN.load(Ordering::Acquire), "native task was stranded");
        #[cfg(not(feature = "tokio-wait-driver"))]
        assert!(perry_runtime::event_pump::loop_statistics().unwrap()[3] > 0);
        perry_runtime::event_pump::shutdown_wait_driver();
        assert!(perry_runtime::event_pump::loop_statistics().is_none());
        perry_runtime::event_pump::js_register_wait_driver(None, None, None);
    }
}
