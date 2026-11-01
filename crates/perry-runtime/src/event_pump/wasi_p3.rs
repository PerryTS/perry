//! A single-agent P3 command parks on the SDK's canonical clock wait.
//! The default P3 libc intentionally traps futex/condvar waits; enabling
//! experimental component threads is unnecessary for JavaScript timers.
use std::time::Duration;

pub(crate) fn sleep(duration: Duration) {
    let mut requested = libc::timespec {
        tv_sec: duration.as_secs().try_into().expect("WASIp3 sleep seconds"),
        tv_nsec: duration.subsec_nanos().into(),
    };
    loop {
        let mut remaining = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: both timespecs are initialized, live stack storage. The
        // SDK waits on wasi:clocks/monotonic-clock@0.3.0, preserving the P3
        // thread-context ABI and acknowledging its subtask before returning.
        if unsafe { libc::nanosleep(&requested, &mut remaining) } == 0 {
            return;
        }
        let error = std::io::Error::last_os_error();
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::Interrupted,
            "WASIp3 clock wait failed: {error}"
        );
        requested = remaining;
    }
}
