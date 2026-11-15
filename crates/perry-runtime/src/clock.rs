//! Numeric wall and performance clocks.
use std::time::{SystemTime, UNIX_EPOCH};
pub(crate) fn wall_millis() -> f64 {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    {
        return crate::snap_census::input_call("clock.wall_millis", &format!("{:?}", ()), || {
            crate::effect!(host_read, clock);
            if let Some(now) = crate::timer::js_mock_timers_date_now() {
                return now;
            }
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as f64)
                .unwrap_or(0.0)
        });
    }
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    {
        #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
        {
            crate::effect!(host_read, clock);
            if let Some(now) = crate::timer::js_mock_timers_date_now() {
                return now;
            }
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as f64)
                .unwrap_or(0.0)
        }
    }
}

/// performance.now() — high-resolution time in milliseconds (sub-ms precision).
/// Returns ms since performance.timeOrigin as f64.
#[no_mangle]
pub extern "C" fn js_performance_now() -> f64 {
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    {
        return crate::snap_census::input_call("clock.performance", &format!("{:?}", ()), || {
            crate::effect!(host_read, clock);
            crate::perf_clock::performance_now_ms()
        });
    }
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    {
        #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
        {
            crate::effect!(host_read, clock);
            crate::perf_clock::performance_now_ms()
        }
    }
}
