//! Opt-in reflection census. These diagnostic counters own no source or heap edges.
use std::sync::{
    atomic::{AtomicU64, Ordering},
    OnceLock,
};

static COUNTERS: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];

#[derive(Clone, Copy)]
pub(crate) enum ReadKind {
    Function,
    Class,
    Info,
    ClassError,
}

#[inline]
pub(crate) fn note(kind: ReadKind) {
    static ENABLED: OnceLock<bool> = OnceLock::new();
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

extern "C" fn report() {
    eprintln!("PERRY_SOURCE_READ_COUNT function_to_string={} class_source={} retained_info={} class_error={}",
        COUNTERS[0].load(Ordering::Relaxed), COUNTERS[1].load(Ordering::Relaxed),
        COUNTERS[2].load(Ordering::Relaxed), COUNTERS[3].load(Ordering::Relaxed));
}
