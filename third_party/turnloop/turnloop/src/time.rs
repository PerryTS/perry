//! Native callers retain std::time::Instant. Browser Wasm uses a timestamp from
//! the backend's monotonic host clock because std's Instant::now is unsupported.
#[cfg(not(turnloop_backend = "web"))]
pub use std::time::Instant;

#[cfg(turnloop_backend = "web")]
mod web {
    use std::{
        ops::{Add, Sub},
        time::Duration,
    };
    /// A timestamp in the backend host clock's monotonic time domain. Obtain it
    /// through Driver::now(); backend adapters construct it from host clock ticks.
    #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
    pub struct Instant(Duration);
    impl Instant {
        /// Construct a timestamp from a monotonic host clock reading.
        pub const fn from_duration(ticks: Duration) -> Self {
            Self(ticks)
        }
        /// Return the underlying host-clock timestamp.
        pub const fn as_duration(self) -> Duration {
            self.0
        }
        /// Add a duration, returning None on timestamp overflow.
        pub fn checked_add(self, duration: Duration) -> Option<Self> {
            self.0.checked_add(duration).map(Self)
        }
        /// Subtract a duration, returning None on timestamp underflow.
        pub fn checked_sub(self, duration: Duration) -> Option<Self> {
            self.0.checked_sub(duration).map(Self)
        }
        /// Elapsed duration since an earlier timestamp, clamped to zero.
        pub fn saturating_duration_since(self, earlier: Self) -> Duration {
            self.0.saturating_sub(earlier.0)
        }
        /// Elapsed duration since an earlier timestamp, clamped to zero.
        pub fn duration_since(self, earlier: Self) -> Duration {
            self.saturating_duration_since(earlier)
        }
    }
    impl Add<Duration> for Instant {
        type Output = Self;
        fn add(self, duration: Duration) -> Self {
            self.checked_add(duration).expect("instant overflow")
        }
    }
    impl Sub<Duration> for Instant {
        type Output = Self;
        fn sub(self, duration: Duration) -> Self {
            self.checked_sub(duration).expect("instant underflow")
        }
    }
    impl Sub for Instant {
        type Output = Duration;
        fn sub(self, earlier: Self) -> Duration {
            self.saturating_duration_since(earlier)
        }
    }
}
#[cfg(turnloop_backend = "web")]
pub use web::Instant;
