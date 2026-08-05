//! Test-only serialization for PROCESS-global state that is neither a runtime
//! side table (those use `gc::global_side_table_test_lock`) nor thread-local.
//!
//! First resident: the process working directory. `std::env::set_current_dir`
//! is process-wide, so a test that changes it (`typed_feedback`'s
//! `CurrentDirGuard`) races every parallel test that reads
//! `std::env::current_dir()` more than once and compares (the `url`
//! path-to-file-URL tests read it once for the expectation and again inside
//! the resolver) — observed as an intermittent
//! `path_to_file_url_posix_does_not_add_slash_without_input_slash` failure
//! under default-parallel `cargo test` (#6965).

/// Serializes tests that MUTATE the process working directory against tests
/// that read it multiple times and compare. Writers hold it for the whole
/// mutation window (guard lifetime); readers hold it across their
/// read-then-compare span. Poison-tolerant: the guarded data is `()`, and one
/// test's failure must not cascade `PoisonError`s into every sibling.
pub(crate) fn process_cwd_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static PROCESS_CWD_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    PROCESS_CWD_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
