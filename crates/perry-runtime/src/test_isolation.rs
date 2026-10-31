//! Tests that assert on process-wide state no lock can quiesce.
//!
//! `NOTIFIED`, the pending-microtask probes, the has-active registries and the
//! stdin listener are single process-wide cells that any of the other ~6000
//! tests may touch (every promise settlement notifies). A test that needs them
//! idle cannot get that from a mutex only its own module takes, so it runs in a
//! child process holding nothing but itself.

/// Run `body` in a fresh process running only the calling test.
///
/// In the parent (the normal test run) this re-executes the test binary with
/// the calling test's exact name and returns once the child passed; in the
/// child (`PERRY_TEST_OWN_PROCESS` set) it runs `body` inline. Call it first
/// in the test, from the test fn itself: the test's name is read off the
/// current thread, which libtest names after the test.
pub(crate) fn in_own_process(body: impl FnOnce()) {
    const MARKER: &str = "PERRY_TEST_OWN_PROCESS";
    if std::env::var_os(MARKER).is_some() {
        body();
        return;
    }
    let thread = std::thread::current();
    let name = thread
        .name()
        .expect("in_own_process must be called from a named libtest thread");
    let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([name, "--exact", "--nocapture"])
        .env(MARKER, "1")
        .output()
        .expect("spawn the isolated test process");
    assert!(
        output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
        "isolated run of {name} failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
