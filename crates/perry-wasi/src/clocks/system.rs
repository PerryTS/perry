// Adapted from Wasmtime 48.0.5 (Bytecode Alliance contributors).
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
// See PROVENANCE.md and LICENSE.

use std::time::{Duration, Instant};

pub trait HostWallClock: Send {
    fn resolution(&self) -> Duration;
}

pub trait HostMonotonicClock: Send {
    fn resolution(&self) -> u64;
    fn now(&self) -> u64;
}

#[derive(Default)]
pub struct WallClock;

impl HostWallClock for WallClock {
    fn resolution(&self) -> Duration {
        #[cfg(unix)]
        {
            let res = rustix::time::clock_getres(rustix::time::ClockId::Realtime);
            Duration::new(
                res.tv_sec.try_into().unwrap(),
                res.tv_nsec.try_into().unwrap(),
            )
        }
        #[cfg(windows)]
        {
            // According to [this blog post], the system timer resolution
            // is 55ms or 10ms. Use the more conservative of the two.
            //
            // [this blog post]: https://devblogs.microsoft.com/oldnewthing/20170921-00/?p=97057
            Duration::new(0, 55_000_000)
        }
    }
}

pub struct MonotonicClock {
    /// The `Instant` this clock was created. All returned times are
    /// durations since that time.
    initial: Instant,
}

impl Default for MonotonicClock {
    fn default() -> Self {
        Self::new()
    }
}

impl MonotonicClock {
    pub fn new() -> Self {
        Self {
            initial: Instant::now(),
        }
    }
}

impl HostMonotonicClock for MonotonicClock {
    fn resolution(&self) -> u64 {
        #[cfg(unix)]
        {
            let res = rustix::time::clock_getres(rustix::time::ClockId::Monotonic);
            u64::try_from(res.tv_sec).unwrap() * 1_000_000_000 + u64::try_from(res.tv_nsec).unwrap()
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Performance::QueryPerformanceFrequency;

            unsafe {
                let mut frequency = 0;
                if QueryPerformanceFrequency(&mut frequency) == 0 {
                    panic!(
                        "QueryPerformanceFrequency failed: {}",
                        std::io::Error::last_os_error()
                    );
                }
                1_000_000_000 / u64::try_from(frequency).unwrap()
            }
        }
    }

    fn now(&self) -> u64 {
        // Unwrap here and in `resolution` above; a `u64` is wide enough to
        // hold over 584 years of nanoseconds.
        Instant::now()
            .duration_since(self.initial)
            .as_nanos()
            .try_into()
            .unwrap()
    }
}

pub fn monotonic_clock() -> Box<dyn HostMonotonicClock + Send> {
    Box::new(MonotonicClock::default())
}

pub fn wall_clock() -> Box<dyn HostWallClock + Send> {
    Box::new(WallClock)
}
