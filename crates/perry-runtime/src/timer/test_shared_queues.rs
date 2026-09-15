//! Cross-thread adoption for the agent timer store, test builds only (#7680).
//!
//! The store is `per_test_global!` (#7674), so by default every libtest thread
//! gets its OWN empty instance. That default is right for the ~180-reader
//! isolation problem #7674 fixed — it is what keeps one test's timers out of
//! another's — but it defeats the one test file whose actual subject IS
//! cross-thread visibility of the SAME store: `agent_dispatch_tests.rs`'s
//! #6185 coverage schedules a timer as the "primary agent" on one thread and
//! spawns a "worker" thread to prove it can neither fire nor see that timer.
//!
//! Under plain per-thread storage the worker's store is empty by construction,
//! so those assertions hold no matter what the per-agent partitioning does.
//! #7680 found that by sabotaging `crate::agent::owns` to always return `true`
//! and watching two of `agent_dispatch_tests`'s five tests still pass. Adopting
//! the primary thread's instance on the worker restores a real shared store, so
//! the partitioning is what the test actually exercises.

use super::store::STORE;

/// This thread's store instance, as an opaque key for [`test_adopt_queues`] on
/// another thread.
pub(crate) fn test_shared_queue_keys() -> usize {
    STORE.shared_key()
}

/// Adopt the store instance obtained from [`test_shared_queue_keys`] on another
/// thread. Must run before this thread's first touch of the store (see
/// [`crate::per_test_global::PerThread::adopt`]).
pub(crate) fn test_adopt_queues(key: usize) {
    STORE.adopt(key);
}
