//! Preallocated indexed timer heap. Allocating comparisons live in turnloop-bench.
use crate::Instant;
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Entry {
    at: Instant,
    id: u64,
}
mod heap;
pub(crate) use heap::Heap as DriverTimerQueue;
pub use heap::Heap;
pub use heap::Heap as TimerQueue;
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn cancel_and_expire_match_sorted_reference() {
        #[cfg(not(turnloop_backend = "web"))]
        let base = Instant::now();
        #[cfg(turnloop_backend = "web")]
        let base = Instant::from_duration(Duration::from_secs(1));
        let mut q = TimerQueue::new(1000);
        let mut reference = Vec::new();
        for i in 0..1000u64 {
            let at = base + Duration::from_nanos((i * 691) % 1000);
            q.insert((1 << 32) | i, at);
            if i % 3 != 0 {
                reference.push((at, (1 << 32) | i));
            }
        }
        for i in (0..1000u64).step_by(3) {
            assert!(q.cancel((1 << 32) | i));
        }
        reference.sort();
        assert_eq!(q.len(), reference.len());
        assert!(q.pop_expired(base - Duration::from_nanos(1)).is_none());
        let mut ran = 0;
        for (at, id) in reference {
            assert_eq!(q.pop_expired(base + Duration::from_secs(1)), Some((id, at)));
            ran += 1;
        }
        assert_eq!(ran, 666);
        assert!(q.is_empty());
    }
}
