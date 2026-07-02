//! Regression test for #5485: an async function must resolve with its own
//! `return` value, not the value of an intermediate `await`.
//!
//! Root cause: `async_step_fulfill_thunk` / `async_step_reject_thunk` (the
//! resume path for `await <pending promise>`) preserved the *ambient*
//! `INLINE_TRAP.trap_next` while pointing `current_step` at the resumed step.
//! When the resumed step belonged to a *nested* activation, its
//! `js_async_step_done` reuse-gate (`current_step == step_closure`) then fired
//! against an *outer* activation's result promise and settled it prematurely
//! with the nested call's (intermediate) value — exactly the hazard
//! `js_async_first_call` already guards against by clearing `trap_next`.
//!
//! Symptom (Hono / Skelpo CMS): `POST /admin/login` returned an empty 200
//! instead of the handler's `302` redirect, because the handler's awaited
//! result resolved to `checkLoginRateLimit()`'s `{ allowed: true }` (the first
//! `await`) instead of the `c.redirect(...)` it actually returned.
//!
//! The fix captures each activation's own `trap_next` as a second closure
//! capture on the resume thunks and restores *that* (not the ambient one).
//!
//! This minimal repro exhibits the leak deterministically via a
//! catch-handler `await` of a nested async fn: on the buggy runtime
//! `tryCatchAwait()` resolves to `{"allowed":true}` (the inner value) instead
//! of `"wrap:{\"allowed\":true}"` (its real return).

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn async_fn_returns_its_own_value_not_intermediate_await() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");

    std::fs::write(
        &entry,
        r#"
const tick = (): Promise<void> => new Promise((r) => setTimeout(() => r(), 0));
async function inner(): Promise<{ allowed: boolean }> {
  await tick();
  return { allowed: true };
}
// The #5485 leak case: a catch-handler `await` of a nested async fn. The
// enclosing async fn must resolve with its OWN return value ("wrap:{...}"),
// not the inner await's intermediate value ({allowed:true}). On the buggy
// runtime the nested call's `js_async_step_done` settled this fn's result
// promise prematurely with {allowed:true}.
async function tryCatchAwait(): Promise<string> {
  try {
    await Promise.reject(new Error("e"));
  } catch {
    const r = await inner();
    return "wrap:" + JSON.stringify(r);
  }
  return "x";
}
async function main(): Promise<void> {
  console.log("B=" + (await tryCatchAwait()));
}
void main();
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
        stdout, "B=wrap:{\"allowed\":true}\n",
        "async fn must resolve with its own return value, not an intermediate await (#5485)"
    );
}
