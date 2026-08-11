//! Cross-thread adoption for the timer queues, test builds only (#7680).
//!
//! `TIMER_QUEUE` / `CALLBACK_TIMERS` / `INTERVAL_TIMERS` are `per_test_global!`
//! (#7674), so by default every libtest thread gets its OWN empty instance.
//! That default is right for the ~180-reader isolation problem #7674 fixed —
//! it is what keeps one test's timers out of another's — but it defeats the
//! one test file whose actual subject IS cross-thread visibility of the SAME
//! queue: `agent_dispatch_tests.rs`'s #6185 coverage schedules a timer as the
//! "primary agent" on one thread and spawns a "worker" thread to prove it can
//! neither fire nor see that timer.
//!
//! Under plain per-thread storage the worker's queue is empty by
//! construction, so those assertions hold no matter what `crate::agent::owns`
//! does. #7680 found this by sabotaging `owns` to always return `true` and
//! watching two of `agent_dispatch_tests`'s five tests still pass — the
//! per-thread split had quietly made them assert nothing about the owner-tag
//! filtering they exist to cover. Adopting the primary thread's instance on
//! the worker restores a real shared queue, so `timer.rs` / `ownership.rs`'s
//! `crate::agent::owns` filtering is what the test actually exercises.

use super::{CALLBACK_TIMERS, INTERVAL_TIMERS, TIMER_QUEUE};

/// This thread's queue instances, as opaque keys for [`test_adopt_queues`] on
/// another thread.
pub(crate) fn test_shared_queue_keys() -> (usize, usize, usize) {
    (
        TIMER_QUEUE.shared_key(),
        CALLBACK_TIMERS.shared_key(),
        INTERVAL_TIMERS.shared_key(),
    )
}

/// Adopt queue instances obtained from [`test_shared_queue_keys`] on another
/// thread. Must run before this thread's first touch of any of the three
/// queues (see [`crate::per_test_global::PerThread::adopt`]).
pub(crate) fn test_adopt_queues(keys: (usize, usize, usize)) {
    TIMER_QUEUE.adopt(keys.0);
    CALLBACK_TIMERS.adopt(keys.1);
    INTERVAL_TIMERS.adopt(keys.2);
}
