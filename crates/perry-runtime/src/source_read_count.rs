//! Opt-in reflection census. These diagnostic counters own no source or heap edges.
use std::sync::{
    atomic::{AtomicU64, Ordering},
    OnceLock,
};

static COUNTERS: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];
static ENABLED: OnceLock<bool> = OnceLock::new();

#[derive(Clone, Copy)]
pub(crate) enum ReadKind {
    Function,
    Class,
    Info,
    ClassError,
    /// An info read that decoded DEFLATE frames (the rest read raw bytes).
    Framed,
}

#[inline]
pub(crate) fn note(kind: ReadKind) {
    let enabled = ENABLED.get_or_init(|| {
        let enabled = matches!(
            std::env::var("PERRY_SOURCE_READ_COUNT").as_deref(),
            Ok("1") | Ok("on") | Ok("true")
        );
        if enabled {
            unsafe {
                libc::atexit(report);
            }
        }
        enabled
    });
    if *enabled {
        COUNTERS[kind as usize].fetch_add(1, Ordering::Relaxed);
    }
}

/// `process.exit()` deliberately skips atexit; retain the same diagnostic there.
pub(crate) fn report_before_immediate_exit() {
    if ENABLED.get().copied().unwrap_or(false) {
        report();
    }
}

extern "C" fn report() {
    eprintln!("PERRY_SOURCE_READ_COUNT function_to_string={} class_source={} retained_info={} class_error={} framed={}",
        COUNTERS[0].load(Ordering::Relaxed), COUNTERS[1].load(Ordering::Relaxed),
        COUNTERS[2].load(Ordering::Relaxed), COUNTERS[3].load(Ordering::Relaxed),
        COUNTERS[4].load(Ordering::Relaxed));
}
