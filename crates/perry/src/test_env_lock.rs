//! Process-wide lock for tests that read or mutate process environment.
//!
//! `std::env::set_var`/`remove_var` mutate state shared by every test thread,
//! so a test that swaps `PATH` races any concurrently-running test that reads
//! it. `optimized_libs::tests` already serialized its own PATH swaps behind a
//! module-private mutex, but `install::lifecycle`'s `run_lifecycle_executes_
//! script` reads `PATH` (via `augment_path`) to resolve `sh` — without sharing
//! that lock it could observe the fake, `sh`-less PATH and fail to spawn with
//! `No such file or directory`.
//!
//! Both sides now take THIS lock, so the guard is genuinely process-wide.
//! Any future test that touches env vars should take it too.

use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

/// Ownership of the process environment for one test.
///
/// A test restores its own changes before it releases the guard. A test that
/// panics never hands the environment on: the workspace builds tests with
/// `panic = "abort"`, so libtest runs every test in a process of its own and a
/// panic ends that process, environment and lock with it.
pub(crate) struct EnvGuard {
    _lock: MutexGuard<'static, ()>,
}

/// Acquire the process-wide environment lock.
pub(crate) fn env_lock() -> EnvGuard {
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let lock = ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    EnvGuard { _lock: lock }
}
