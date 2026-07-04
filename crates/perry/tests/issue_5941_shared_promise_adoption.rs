//! Regression (#5941): the Next.js dynamic-RSC render deadlock's two runtime
//! roots, each with a minimal repro.
//!
//! 1. Shared-promise adoption severing: `js_promise_resolve_with_promise`'s
//!    slow path unconditionally nulled `inner.next` after parking its
//!    forwarders in the overflow table. `inner.next` can hold a PRIOR
//!    dependent's fast-path adoption edge (the first adoption of a
//!    reaction-free inner chains its outer there), so a SECOND adoption of
//!    the same shared inner promise severed the first dependent's only
//!    edge — its promise never settled. At Next.js App Router scale the
//!    render chain's async-step machines adopt shared React work promises
//!    in exactly this shape; the severed machine's result parked the whole
//!    await graph (the request-time dynamic routes hung with 18 suspended
//!    machines).
//!
//! 2. Swallowed step-resume rejection: an async-step machine resumed via
//!    the promise thunks that exits through its step body's internal catch
//!    arm (`throw` after a pending `await`) returned a FRESH rejected
//!    promise that the thunks discarded — the machine's real result
//!    promise (the captured per-activation `trap_next`, #5485) stayed
//!    Pending and the caller's `catch` never ran.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Compile `src` and run it. Returns (exit_ok, stdout).
fn compile_and_run(src: &str) -> (bool, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(&entry, src).expect("write entry");
    let output = dir.path().join("main_bin");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .args([
            "compile",
            entry.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ])
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    (
        run.status.success(),
        String::from_utf8_lossy(&run.stdout).to_string(),
    )
}

/// Two async fns adopting ONE shared pending promise: the first adoption
/// takes resolve_with_promise's fast path (`shared.next = resultA`), the
/// second takes the slow path — which used to null `shared.next`,
/// orphaning `pa` forever (the run then exits non-zero via the
/// unsettled-top-level-await detector instead of printing).
#[test]
fn second_adoption_of_shared_promise_preserves_first_adopters_chain() {
    let (ok, stdout) = compile_and_run(
        r#"
let release: (v: string) => void;
const shared = new Promise<string>((res) => {
  release = res;
});
// `await 0` forces a real state transition so js_async_step_done runs
// with a live trap_next and ADOPTS `shared` (without it, Promise.resolve
// identity short-circuits and no adoption happens).
async function a(): Promise<string> {
  await 0;
  return shared;
}
async function b(): Promise<string> {
  await 0;
  return shared;
}
const pa = a();
const pb = b();
setTimeout(() => release!("done"), 10);
const ra = await pa;
const rb = await pb;
console.log(ra, rb);
"#,
    );
    assert!(ok, "run must exit cleanly (first adopter's await hung)");
    assert_eq!(stdout, "done done\n");
}

/// A throw AFTER a pending await must reject the async fn's result so the
/// caller's catch runs (the resume thunks used to discard the step's
/// catch-arm `Promise.reject` return, hanging the caller instead).
#[test]
fn throw_after_pending_await_rejects_the_result_promise() {
    let (ok, stdout) = compile_and_run(
        r#"
async function boom(): Promise<string> {
  await new Promise<void>((r) => setTimeout(r, 10));
  throw new Error("x");
}
async function outer(): Promise<void> {
  try {
    await boom();
    console.log("no-throw");
  } catch (e) {
    console.log("caught", (e as Error).message);
  }
}
await outer();
"#,
    );
    assert!(ok, "run must exit cleanly (caller's catch never ran)");
    assert_eq!(stdout, "caught x\n");
}
