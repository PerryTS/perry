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

use std::sync::{Mutex, MutexGuard, OnceLock};

/// Acquire the process-wide environment lock.
///
/// Fails closed on poisoning, matching the guard this replaced. Callers restore
/// the environment *after* the work they wrap, so a panic mid-test unwinds
/// without restoring — leaving `PATH` pointing at a deleted temp dir. Recovering
/// the lock there would hand that broken environment to every subsequent test
/// and turn one failure into a cascade that no longer names its cause. Poison
/// means "a test that owns the environment died"; stopping is the useful answer.
pub(crate) fn env_lock() -> MutexGuard<'static, ()> {
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("env lock poisoned — a test panicked while owning the process environment")
}
