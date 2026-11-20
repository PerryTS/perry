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

/// Run a libtest case that needs exclusive ownership of process-global state
/// (#9197). A lock shared by only a few tests cannot exclude the runtime's
/// other side-table readers or counters; owning the process can.
///
/// Every test already owns its process: the workspace runs tests with
/// `panic-abort-tests`, and libtest spawns one fresh process per test (see
/// [`current_test_name`]). So the body runs inline, after checking that this
/// is that process.
pub(crate) fn isolated_test(body: impl FnOnce()) {
    let _ = current_test_name();
    body();
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
/// Always `Some` in libtest's per-test process. For tests that read a
/// process-wide counter as a before/after delta (`PAYLOAD_FINALIZED`,
/// `LIVE_BACKINGS`): no lock shared by a few tests can stop an unrelated test
/// from moving such a counter, only owning the process can.
pub(crate) fn isolated_process_guard() -> Option<IsolatedProcess> {
    let _ = current_test_name();
    Some(IsolatedProcess)
}

/// Held by a test body that owns its process ([`isolated_process_guard`]).
#[must_use]
pub(crate) struct IsolatedProcess;

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

// ---------------------------------------------------------------------------
// The per-test process (the workspace's `panic-abort-tests`).
//
// The workspace builds tests with `panic = "abort"`, so libtest runs EVERY test
// in a fresh process of its own: the runner spawns the test binary with
// `__RUST_TEST_INVOKE=<test name>` and no arguments; the child removes that
// variable, runs exactly that one test on its main thread, exits 50 (libtest's
// `TR_OK`) when it passes and aborts when it panics. A panic therefore never returns to the test
// (`catch_unwind` cannot see one), and a test that must observe a panic does it
// from a second process: [`test_process`] starts this same test again and
// [`expect_child_panic`] asserts on how that process died.

/// The name libtest handed this process, captured by a constructor before
/// `main` (libtest removes `__RUST_TEST_INVOKE` before the test runs). Empty
/// in the runner process itself.
static TEST_NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// The variable libtest's runner sets to name the one test a per-test process
/// runs.
pub(crate) const TEST_INVOKE: &str = "__RUST_TEST_INVOKE";

/// The exit code of a per-test process whose test passed (libtest's `TR_OK`;
/// `library/test/src/test_result.rs`). Not 0: libtest's runner reads 0 as a
/// test that exited without reporting.
pub(crate) const TEST_PASSED: i32 = 50;

extern "C" fn capture_test_name() {
    if let Some(name) = std::env::var_os(TEST_INVOKE) {
        let _ = TEST_NAME.set(name.to_string_lossy().into_owned());
    }
}

#[used]
#[cfg_attr(
    any(target_os = "linux", target_os = "android", target_os = "freebsd"),
    link_section = ".init_array"
)]
#[cfg_attr(
    any(target_os = "macos", target_os = "ios"),
    link_section = "__DATA,__mod_init_func"
)]
#[cfg_attr(target_os = "windows", link_section = ".CRT$XCU")]
static CAPTURE_TEST_NAME: extern "C" fn() = capture_test_name;

/// The full libtest name of the test this process runs (`module::path::test`).
/// Panics outside a per-test process: every caller relies on owning the
/// process, and a runner or a `--force-run-in-process` run does not.
pub(crate) fn current_test_name() -> &'static str {
    TEST_NAME.get().map(String::as_str).unwrap_or_else(|| {
        panic!("not in libtest's per-test process (`panic-abort-tests`): no __RUST_TEST_INVOKE at startup")
    })
}

/// A command that runs the test `name` alone in a fresh process, exactly as
/// libtest does: on that process's main thread, output not captured, exit
/// [`TEST_PASSED`] on a pass and SIGABRT on a panic. The caller adds its own
/// env markers.
pub(crate) fn test_process(name: &str) -> std::process::Command {
    let mut command =
        std::process::Command::new(std::env::current_exe().expect("current test binary"));
    command.env(TEST_INVOKE, name);
    command
}

/// [`test_process`] for the test this process runs.
pub(crate) fn this_test_process() -> std::process::Command {
    test_process(current_test_name())
}

/// Did `status` end the way a passing test process ends ([`TEST_PASSED`])?
pub(crate) fn passed(status: std::process::ExitStatus) -> bool {
    status.code() == Some(TEST_PASSED)
}

/// Did `status` end the way a panicking test process ends (`process::abort`)?
pub(crate) fn died_of_abort(status: std::process::ExitStatus) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status.signal() == Some(libc::SIGABRT)
    }
    #[cfg(windows)]
    {
        // `process::abort` is `__fastfail(FAST_FAIL_FATAL_APP_EXIT)`.
        status.code() == Some(0xC000_0409_u32 as i32)
    }
    #[cfg(not(any(unix, windows)))]
    {
        // No child processes to inspect (wasm32): nothing died of an abort.
        let _ = status;
        false
    }
}

/// The panic message the default hook wrote to `stderr` (the text after the
/// first `panicked at <location>:` line).
pub(crate) fn panic_message_in(stderr: &str) -> Option<String> {
    let start = stderr.find("panicked at ")?;
    let rest = &stderr[start..];
    let body = &rest[rest.find('\n')? + 1..];
    Some(body.trim_end().to_string())
}

const EXPECT_PANIC_CALL: &str = "PERRY_EXPECT_PANIC_CALL";
const EXPECT_PANIC_PRIOR: &str = "PERRY_EXPECT_PANIC_PRIOR";

/// `(message, at-panic report)` of every expected panic this process has seen.
static EXPECT_PANIC_CALLS: std::sync::Mutex<Vec<(String, String)>> =
    std::sync::Mutex::new(Vec::new());

fn hex_encode(text: &str) -> String {
    text.bytes().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(hex: &str) -> String {
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex panic message"))
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Assert that `f` panics with a message containing `expected`; return the
/// whole message.
///
/// The test's process never runs `f`: a panic would abort it. The calls are
/// numbered in program order; for call `n` a fresh process of the same test
/// runs up to call `n` (the earlier calls hand back the messages this process
/// already saw, without running their `f`), runs `f`, and exits 3 if `f`
/// returns. This process asserts that the child aborted (a panic) and that its
/// message contains `expected`. What the test does after the call sees the
/// state from before `f`: `f`'s partial effects end with the child (use
/// [`expect_child_panic_then`] to assert on them).
#[track_caller]
pub(crate) fn expect_child_panic<R>(expected: &str, f: impl FnOnce() -> R) -> String {
    expect_child_panic_at(std::panic::Location::caller(), expected, f, None).0
}

/// [`expect_child_panic`] that also reports the state `f` left at its panic:
/// the child runs `at_panic` from its panic hook, before the process aborts,
/// and this returns `(message, at_panic's report)`. Under `panic = "abort"`
/// nothing runs between a panic and the end of the process, so that state is
/// the state the panic leaves in a shipped build.
#[track_caller]
pub(crate) fn expect_child_panic_then<R>(
    expected: &str,
    f: impl FnOnce() -> R,
    at_panic: impl Fn() -> String + Send + Sync + 'static,
) -> (String, String) {
    let (message, report) = expect_child_panic_at(
        std::panic::Location::caller(),
        expected,
        f,
        Some(Box::new(at_panic)),
    );
    (message, report.expect("at-panic report"))
}

const AT_PANIC_REPORT: &str = "perry expect_child_panic at-panic: ";

type AtPanic = Box<dyn Fn() -> String + Send + Sync + 'static>;

fn expect_child_panic_at<R>(
    caller: &'static std::panic::Location<'static>,
    expected: &str,
    f: impl FnOnce() -> R,
    at_panic: Option<AtPanic>,
) -> (String, Option<String>) {
    let mut calls = EXPECT_PANIC_CALLS.lock().unwrap_or_else(|p| p.into_inner());
    let index = calls.len();
    if let Some(target) = std::env::var_os(EXPECT_PANIC_CALL) {
        let target: usize = target
            .to_str()
            .and_then(|s| s.parse().ok())
            .expect("call index");
        if index == target {
            drop(calls);
            if let Some(at_panic) = at_panic {
                let previous = std::panic::take_hook();
                std::panic::set_hook(Box::new(move |info| {
                    eprintln!("{AT_PANIC_REPORT}{}", at_panic().replace('\n', " "));
                    previous(info);
                }));
            }
            let _ = f();
            eprintln!("expect_child_panic: call {index} at {caller} returned without panicking");
            std::process::exit(3);
        }
        let prior = std::env::var(EXPECT_PANIC_PRIOR).unwrap_or_default();
        let (message, report) = prior
            .split(',')
            .nth(index)
            .and_then(|pair| pair.split_once('.'))
            .map(|(message, report)| (hex_decode(message), hex_decode(report)))
            .expect("an earlier expected panic's message");
        calls.push((message.clone(), report.clone()));
        return (message, at_panic.map(|_| report));
    }
    let prior: Vec<String> = calls
        .iter()
        .map(|(message, report)| format!("{}.{}", hex_encode(message), hex_encode(report)))
        .collect();
    let output = this_test_process()
        .env(EXPECT_PANIC_CALL, index.to_string())
        .env(EXPECT_PANIC_PRIOR, prior.join(","))
        .output()
        .expect("spawn the expected-panic test process");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        died_of_abort(output.status),
        "expected a panic containing {expected:?} at {caller}, the child ended {}:\n{stderr}",
        output.status
    );
    let message = panic_message_in(&stderr)
        .unwrap_or_else(|| panic!("child at {caller} aborted without a panic message:\n{stderr}"));
    assert!(
        message.contains(expected),
        "panic message {message:?} at {caller} did not contain {expected:?}"
    );
    let report = at_panic.map(|_| {
        stderr
            .lines()
            .find_map(|line| line.strip_prefix(AT_PANIC_REPORT))
            .unwrap_or_else(|| panic!("child at {caller} printed no at-panic report:\n{stderr}"))
            .to_string()
    });
    calls.push((message.clone(), report.clone().unwrap_or_default()));
    (message, report)
}

/// `catch_unwind` for a closure that must panic: `Err` carries the panic
/// message (a `String` payload) of `f` run in a child process, as
/// [`expect_child_panic`] describes. A closure that returns fails the calling
/// test, as a `catch_unwind(..).is_err()` assertion would.
#[track_caller]
pub(crate) fn child_unwind<R>(f: impl FnOnce() -> R) -> std::thread::Result<R> {
    Err(Box::new(expect_child_panic("", f)))
}

/// Run `f` on a fresh thread and join it: `Err` carries the panic message (a
/// `String` payload). A panic aborts the process it happens in, so when the
/// caller expects one (`panics`) the thread runs in a child process
/// ([`child_unwind`]); otherwise in this process, where a panic fails the
/// test.
#[track_caller]
pub(crate) fn thread_outcome<R: Send + 'static>(
    panics: bool,
    f: impl FnOnce() -> R + Send + 'static,
) -> std::thread::Result<R> {
    let run = move || {
        std::thread::spawn(f)
            .join()
            .unwrap_or_else(|_| unreachable!("a panic aborts the process"))
    };
    if panics {
        child_unwind(run)
    } else {
        Ok(run())
    }
}
