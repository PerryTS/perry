//! Test-only isolation and serialization for process-global fixtures.
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

/// Run a libtest case in its own process when its fixture needs exclusive
/// ownership of process-global state (#9197). A lock shared by only a few
/// tests cannot exclude the runtime's other side-table readers or counters.
///
/// Use the harness's current test name so renaming/moving a test cannot leave
/// a stale filter. Require a marker emitted AFTER the body as well as a clean
/// exit: selecting zero tests or exiting early must not produce a false pass.
pub(crate) fn isolated_test(body: impl FnOnce()) {
    const CHILD_ENV: &str = "PERRY_RUNTIME_ISOLATED_TEST_NAME";
    let thread = std::thread::current();
    let name = thread.name().expect("libtest must name the test thread");
    let completed = format!("perry isolated test completed: {name}");
    if std::env::var(CHILD_ENV).ok().as_deref() == Some(name) {
        body();
        println!("{completed}");
        return;
    }

    let output = std::process::Command::new(std::env::current_exe().expect("current test binary"))
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, name)
        .output()
        .expect("launch isolated runtime test");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.lines().any(|line| line.ends_with(&completed)),
        "isolated test {name} did not complete: {}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status
    );
}
