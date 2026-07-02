//! Regression test for #5591: the `js_native_call_method` recursion-depth
//! guard leaked across thrown-and-caught exceptions.
//!
//! Method dispatch is wrapped in a RAII `CallMethodDepthGuard` (a stack-overflow
//! backstop for circular module init). Perry's exceptions unwind with
//! `setjmp`/`longjmp`, which does NOT run Rust `Drop`s — so every exception
//! thrown from inside a method call and caught by an outer `try` skipped the
//! guard's decrement, leaking one count per throw/catch. After
//! `MAX_CALL_METHOD_DEPTH` (512) such cycles the guard tripped *permanently* and
//! every subsequent method call returned the empty null-object fallback instead
//! of dispatching — so a method that should throw (or compute) silently no-op'd.
//!
//! This surfaced in test262's `%TypedArray%.prototype.{every,some}`
//! callbackfn-not-callable cases: each runs ~11 `assert.throws` cycles per typed
//! array across every constructor, blowing past 512 cumulative throws mid-suite,
//! after which the next non-callable `every(NaN)` stopped throwing.
//!
//! The fix snapshots the dispatch depth at each `try` (`js_try_push`) and
//! restores it on the `longjmp` unwind (`js_throw`), mirroring the existing
//! shadow-stack / async-context restore. This test pins that a long
//! throw-and-catch loop through method dispatch leaves dispatch fully working.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn method_dispatch_survives_many_caught_throws() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");

    std::fs::write(
        &entry,
        r#"
const u = new Uint8Array([10, 20]);

// Throw-and-catch through method dispatch far more than MAX_CALL_METHOD_DEPTH
// (512) times. Each `.every(NaN)` must throw a TypeError (NaN is not callable);
// with the pre-fix depth leak the throws stopped firing partway through.
let caught = 0;
for (let i = 0; i < 2000; i++) {
  try {
    (u as any).every(NaN);
  } catch (e) {
    if (!(e instanceof TypeError)) throw e;
    caught++;
  }
}
console.log("caught=" + caught);

// After the loop, method dispatch must STILL work — both the throwing path and
// ordinary value-returning calls. Pre-fix, the guard was wedged and these
// returned the empty null-object fallback (no throw / wrong value).
let stillThrows = false;
try {
  (u as any).every(NaN);
} catch (e) {
  if (!(e instanceof TypeError)) throw e;
  stillThrows = true;
}
console.log("stillThrows=" + stillThrows);

// A valid callback must still iterate and compute correctly.
console.log("every=" + (u as any).every((x: number) => x >= 10));
console.log("map=" + Array.from((u as any).map((x: number) => x + 1)).join(","));
console.log("len=" + (u as any).length);
"#,
    )
    .expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
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
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        stdout,
        "caught=2000\n\
         stillThrows=true\n\
         every=true\n\
         map=11,21\n\
         len=2\n",
        "method dispatch must keep working after >512 caught throws — the \
         depth guard must not leak across longjmp unwinds (#5591)"
    );
}
