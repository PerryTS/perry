//! Shared process performance-clock authority.
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
struct PerfClock {
    monotonic_start: Instant,
    time_origin_ms: f64,
}

/// Shared clock for `performance.timeOrigin` and `performance.now()`.
///
/// `init_time_origin()` is called from runtime initialization so user code
/// observes a process-start origin. The `OnceLock` fallback keeps direct unit
/// tests and unusual embedder paths well-defined.
static PERF_CLOCK: OnceLock<PerfClock> = OnceLock::new();

fn wall_clock_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

fn perf_clock() -> &'static PerfClock {
    crate::once_init::get_or_init(&PERF_CLOCK, || PerfClock {
        monotonic_start: Instant::now(),
        time_origin_ms: wall_clock_ms(),
    })
}

pub(crate) fn init_time_origin() {
    let _ = perf_clock();
}

pub(crate) fn time_origin_ms() -> f64 {
    perf_clock().time_origin_ms
}

pub(crate) fn performance_now_ms() -> f64 {
    perf_clock().monotonic_start.elapsed().as_secs_f64() * 1000.0
}
