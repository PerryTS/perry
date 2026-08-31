//! #9305: a JS throw inside a microtask longjmp-lands in the microtask
//! runner's trap. Pre-fix the runner armed `setjmp` directly from Rust —
//! rustc cannot express `returns_twice`, so LLVM colored the spilled
//! TLS-base temporary's stack slot into the task-record copy loop, and the
//! landing reloaded a clobbered slot (SIGSEGV, NULL TLS base). The trap now
//! arms inside the C trampoline `perry_sjlj_try` (exception.rs
//! `arm_trap_and_run`), which is immune by construction.
//!
//! NOTE on coverage: the miscompile is an optimized-build phenomenon — the
//! coloring exists in the release-profile runtime archive (it was
//! app-independent: the same libperry_runtime.a crashed every program that
//! threw from a microtask after one task pop). When this test runs against
//! a debug runtime it still pins the routing behavior (rejection of the
//! chained promise, byte-identical drain order vs node); against a release
//! runtime it is the crash regression test.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

#[test]
fn throw_in_microtask_lands_safely_and_matches_node() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = workspace_root().join("test-files/test_issue_9305_throw_in_microtask.ts");
    let output = dir.path().join("main_bin");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-auto-optimize")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled program failed (#9305 regression: a longjmp landing in the \
         microtask runner read a colored stack slot)\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    // Golden captured from node (v26): microtask FIFO order is deterministic.
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "sync-done\ninner-caught:inner\nbenign:2\ncaught:boom-9305\nrecaught:re:first\nsecond:second-landing\n"
    );
}
