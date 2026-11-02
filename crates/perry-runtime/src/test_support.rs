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

/// Serializes tests that `dlopen`/`dlclose` an object against tests that read
/// every loaded object's tables or assert the eh walker's per-image cache. A
/// reader that enumerated the images while a writer had an object loaded
/// would otherwise read its tables after the `dlclose` unmapped them (a
/// SIGSEGV in `direct_rows_match_gimli_across_every_fde`), or see its cache
/// dropped by the generation change. Poison-tolerant like the cwd lock.
pub(crate) fn loaded_objects_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOADED_OBJECTS_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOADED_OBJECTS_TEST_LOCK
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

/// Marker line the isolated child prints when its test body has finished.
#[must_use]
pub(crate) struct IsolatedDone(String);

impl Drop for IsolatedDone {
    fn drop(&mut self) {
        println!("perry isolated test completed: {}", self.0);
    }
}

/// Statement form of [`isolated_test`], for a test whose body should not be
/// re-indented into a closure:
///
/// ```ignore
/// let Some(_isolated) = crate::test_support::isolated_process_guard() else {
///     return;
/// };
/// ```
///
/// In the parent (the ordinary parallel run) this re-executes the test binary
/// for just this test (`--exact`, one thread), asserts the child succeeded and
/// printed its completion marker, and returns `None` so the caller returns. In
/// the child it returns a guard that prints the marker when the body ends, so
/// the parent can tell a body that ran from a filter that selected nothing.
/// For tests that read a process-wide counter as a before/after delta
/// (`PAYLOAD_FINALIZED`, `LIVE_BACKINGS`): no lock shared by a few tests can
/// stop an unrelated test from moving such a counter, only owning the process
/// can.
pub(crate) fn isolated_process_guard() -> Option<IsolatedDone> {
    const CHILD_ENV: &str = "PERRY_RUNTIME_ISOLATED_TEST_NAME";
    let thread = std::thread::current();
    let name = thread.name().expect("libtest must name the test thread");
    if std::env::var(CHILD_ENV).ok().as_deref() == Some(name) {
        return Some(IsolatedDone(name.to_string()));
    }
    isolated_test(|| unreachable!("the parent never runs the body"));
    None
}

std::thread_local! {
    static PRISTINE_ARENA_FIXTURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Per-thread runtime bootstrap for test threads. Runtime state is created
/// lazily per thread and libtest gives every case a fresh thread, so without
/// this a fixture that reaches a producer before any `state()` call sees
/// closures without a base shape, undefined layout-born fields, or aborts on an
/// unregistered layout. Called from the arena's slow allocation path, which the
/// first allocation on any thread always reaches, so it is the one place every
/// fixture passes through, locked or not. A no-op once the thread has state.
///
/// Fixtures that assert exact byte counts on a pristine arena opt out with
/// [`opt_out_of_thread_bootstrap`] (seeding places the static atoms in it).
pub(crate) fn bootstrap_test_thread() {
    if !PRISTINE_ARENA_FIXTURE.with(|c| c.get()) {
        let _ = crate::state::state();
    }
}

/// Keep this thread's arena unseeded until the fixture asks for `state()`.
pub(crate) fn opt_out_of_thread_bootstrap() {
    PRISTINE_ARENA_FIXTURE.with(|c| c.set(true));
}

/// Restores the two process-wide array-prototype latches a test may set by
/// retargeting a real array's prototype (`ARRAY_TARGET_PROTO_RECORDED` and the
/// array-index fast-path invalidation). Left latched they stand
/// `plain_array_index_guard` down for every later typed-feedback guard test in
/// the process. Takes the typed-feedback test lock for the window, as
/// `dyn_eval`'s guard of the same name does; take it before other locks.
pub(crate) struct ArrayPrototypeLatchGuard {
    _guard_tests: std::sync::MutexGuard<'static, ()>,
    latch: bool,
    invalidated: u8,
}

impl ArrayPrototypeLatchGuard {
    pub(crate) fn new() -> Self {
        let _guard_tests = crate::typed_feedback::typed_feedback_test_lock();
        Self {
            _guard_tests,
            latch: crate::object::prototype_chain::array_static_proto_recorded(),
            invalidated: crate::array::PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED
                .load(std::sync::atomic::Ordering::Relaxed),
        }
    }
}

impl Drop for ArrayPrototypeLatchGuard {
    fn drop(&mut self) {
        crate::object::prototype_chain::test_swap_array_static_proto_recorded(self.latch);
        crate::array::test_swap_array_index_fast_path_invalidated(self.invalidated);
    }
}
