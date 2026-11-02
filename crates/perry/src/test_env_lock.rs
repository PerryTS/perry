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

use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

/// Ownership of the process environment for one test.
///
/// The guard records the environment when it is taken. A test restores its own
/// changes when it passes; when it panics halfway through its swaps, the guard
/// puts the recorded environment back while unwinding, before the lock is
/// released. The next owner therefore always starts from the environment the
/// previous one was given, which makes a poisoned lock harmless: the panic is
/// reported by the test that panicked, and every other test runs against a
/// clean environment instead of failing on the poison (one failing test used
/// to fail every later env-lock test with it). Only the unwinding guard
/// restores: tests that set variables without this lock run concurrently, and
/// a restore on every release would undo their changes under them.
pub(crate) struct EnvGuard {
    saved: Vec<(OsString, OsString)>,
    _lock: MutexGuard<'static, ()>,
}

/// Acquire the process-wide environment lock.
pub(crate) fn env_lock() -> EnvGuard {
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let lock = ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    EnvGuard {
        saved: std::env::vars_os().collect(),
        _lock: lock,
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            return;
        }
        for (key, _) in std::env::vars_os() {
            if !self.saved.iter().any(|(saved, _)| *saved == key) {
                std::env::remove_var(&key);
            }
        }
        for (key, value) in &self.saved {
            if std::env::var_os(key).as_ref() != Some(value) {
                std::env::set_var(key, value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::env_lock;

    const KEY: &str = "TEST_ENV_LOCK_RESTORE_PROBE";

    /// A test that panics while it owns the environment leaves neither its
    /// changes nor a poisoned lock to the next owner.
    #[test]
    fn a_panicking_owner_leaves_the_environment_and_the_lock_usable() {
        let before = {
            let _env = env_lock();
            std::env::var_os(KEY)
        };
        let panicked = std::thread::spawn(|| {
            let _env = env_lock();
            std::env::set_var(KEY, "dirty");
            panic!("owner dies with the environment changed");
        })
        .join();
        assert!(panicked.is_err(), "the owner must have panicked");
        let _env = env_lock();
        assert_eq!(
            std::env::var_os(KEY),
            before,
            "the panicking owner's change must be undone"
        );
    }
}
