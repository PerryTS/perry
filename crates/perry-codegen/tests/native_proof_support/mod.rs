//! Shared harness for the `native_proof_*` suites' artifact-JSON tests
//! (#7493).
//!
//! `PERRY_NATIVE_REPS` / `PERRY_NATIVE_REPS_DIR` are read by `compile_module`
//! from the **process** environment, so they are global to a whole test binary
//! rather than to the test that set them. Both suites had a hand-rolled copy of
//! the same set-compile-restore dance, and both copies had the same two
//! defects. They are fixed once, here, and included by both binaries with
//! `#[path]` so a third copy cannot drift.
//!
//! Defect 1 — **poisoning cascade** (#7490's shape, fixed for
//! `typed_feedback.rs` in #7492). A test that unwinds while holding the lock
//! poisons it, and every later `lock().unwrap()` in the binary then dies with
//! `PoisonError` regardless of its own subject. On `main`,
//! `native_proof_regressions` reported **55 failures at default parallelism and
//! 4 under `--test-threads=1`** — 51 of the 55 were `PoisonError`, and *which*
//! tests they hit was decided by the scheduler. That reads as order-dependent
//! codegen state when it is only lock poisoning.
//!
//! Defect 2 — **env leak on unwind**. The restore was hand-written after the
//! compile, so a panic inside `compile_module` left `PERRY_NATIVE_REPS=1` and a
//! stale `PERRY_NATIVE_REPS_DIR` installed for the rest of the process. Every
//! later compile in the binary — including the many `compile_ir` ones that
//! never take this lock — then wrote artifact JSON into a directory another
//! test was reading, producing the torn `EOF while parsing a value` read that
//! poisoned the mutex in the first place.
//!
//! Each including binary gets its own `ARTIFACT_ENV_LOCK` static and its own
//! copy of the two tests below, which is what we want: the lock is per-process
//! and so is the property being asserted.

#![allow(dead_code)]

pub static ARTIFACT_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Acquire [`ARTIFACT_ENV_LOCK`] tolerating a poisoned mutex.
///
/// Recovery is sound because the protected state — the `PERRY_NATIVE_REPS*`
/// env vars — is restored by [`NativeRepsEnv`]'s `Drop` during the same unwind,
/// before the mutex is released. One test's failure must fail that test alone.
pub fn artifact_env_lock() -> std::sync::MutexGuard<'static, ()> {
    ARTIFACT_ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// RAII for the `PERRY_NATIVE_REPS*` env vars.
pub struct NativeRepsEnv {
    reps: Option<std::ffi::OsString>,
    dir: Option<std::ffi::OsString>,
    all_typed_clone_rejections: Option<std::ffi::OsString>,
}

impl NativeRepsEnv {
    /// Install the artifact-recording env for the lifetime of the guard.
    /// Hold [`artifact_env_lock`] across this — the vars are process-global.
    pub fn install(dir: &std::path::Path, all_typed_clone_rejections: bool) -> Self {
        let saved = NativeRepsEnv {
            reps: std::env::var_os("PERRY_NATIVE_REPS"),
            dir: std::env::var_os("PERRY_NATIVE_REPS_DIR"),
            all_typed_clone_rejections: std::env::var_os(
                "PERRY_NATIVE_REPS_ALL_TYPED_CLONE_REJECTIONS",
            ),
        };
        std::env::set_var("PERRY_NATIVE_REPS", "1");
        std::env::set_var("PERRY_NATIVE_REPS_DIR", dir);
        if all_typed_clone_rejections {
            std::env::set_var("PERRY_NATIVE_REPS_ALL_TYPED_CLONE_REJECTIONS", "1");
        } else {
            std::env::remove_var("PERRY_NATIVE_REPS_ALL_TYPED_CLONE_REJECTIONS");
        }
        saved
    }
}

impl Drop for NativeRepsEnv {
    fn drop(&mut self) {
        fn restore(key: &str, value: Option<&std::ffi::OsString>) {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        restore("PERRY_NATIVE_REPS", self.reps.as_ref());
        restore("PERRY_NATIVE_REPS_DIR", self.dir.as_ref());
        restore(
            "PERRY_NATIVE_REPS_ALL_TYPED_CLONE_REJECTIONS",
            self.all_typed_clone_rejections.as_ref(),
        );
    }
}

/// Pick this test's own artifact out of `dir`, tolerating foreign or
/// half-written neighbours.
///
/// `PERRY_NATIVE_REPS_DIR` is process-global while installed, so a test
/// compiling concurrently on another thread — none of which take the lock —
/// also drops its artifact here, possibly still being written when we read.
/// Treat that as noise (it cannot be the subject) instead of unwrapping a torn
/// read into a panic INSIDE the lock, which is what poisons it. If the subject
/// then turns out to be missing, the panic names everything that was skipped,
/// so a genuinely truncated target artifact stays diagnosable.
pub fn artifact_for_module(dir: &std::path::Path, name: &str) -> serde_json::Value {
    let paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    let mut skipped = Vec::new();
    for path in paths {
        if !path.extension().is_some_and(|ext| ext == "json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            skipped.push(format!("{}: unreadable", path.display()));
            continue;
        };
        let value: serde_json::Value = match serde_json::from_str(&text) {
            Ok(value) => value,
            Err(err) => {
                skipped.push(format!("{}: {err}", path.display()));
                continue;
            }
        };
        if value["module"] == name {
            return value;
        }
        skipped.push(format!("{}", value["module"]));
    }
    panic!("native reps artifact for {name} not found in {dir:?}; skipped {skipped:?}");
}

/// Sabotage test for [`artifact_env_lock`]: a test that panics while holding
/// the lock must fail *itself* and nothing else.
///
/// This asserts its subject was live rather than merely that nothing threw — it
/// plants the exact #7490 shape, proves the mutex really is poisoned
/// afterwards, and only then demands the accessor still hand out a guard. It
/// fails against a plain `ARTIFACT_ENV_LOCK.lock().unwrap()`, so a green run
/// here is evidence.
///
/// `a` sorts first, so under `--test-threads=1` every other test in the
/// including binary runs under a genuinely poisoned lock and the tolerance is
/// exercised suite-wide rather than in one isolated case.
#[test]
fn artifact_env_lock_is_poison_tolerant_so_one_failure_cannot_cascade() {
    let sabotage = std::panic::catch_unwind(|| {
        let _guard = artifact_env_lock();
        panic!("#7493 sabotage: unwinding while holding ARTIFACT_ENV_LOCK");
    });
    assert!(sabotage.is_err(), "the sabotage panic should have unwound");
    assert!(
        ARTIFACT_ENV_LOCK.is_poisoned(),
        "unwinding out of a lock-holding test should poison ARTIFACT_ENV_LOCK — \
         if it no longer does, this test is no longer exercising its subject"
    );
    let _guard = artifact_env_lock();
}

/// Sabotage test for [`NativeRepsEnv`]: the env must survive an unwind out of
/// the compile it wraps.
#[test]
fn artifact_env_is_restored_even_when_the_compile_unwinds() {
    let _guard = artifact_env_lock();
    let before = std::env::var_os("PERRY_NATIVE_REPS");
    let sabotage = std::panic::catch_unwind(|| {
        let _env = NativeRepsEnv::install(std::path::Path::new("/nonexistent/perry7493"), false);
        assert_eq!(
            std::env::var("PERRY_NATIVE_REPS").ok().as_deref(),
            Some("1"),
            "the guard must actually install the var it claims to restore"
        );
        panic!("#7493 sabotage: unwinding inside the artifact env window");
    });
    assert!(sabotage.is_err(), "the sabotage panic should have unwound");
    assert_eq!(
        std::env::var_os("PERRY_NATIVE_REPS"),
        before,
        "PERRY_NATIVE_REPS leaked out of a panicking compile window"
    );
}
