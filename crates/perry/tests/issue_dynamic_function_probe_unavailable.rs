//! A trivial no-op `new Function("")` / `Function("")` is the canonical
//! runtime-dynamic-code-generation capability probe:
//!
//! ```js
//! const allowsEval = (() => { try { return (new Function(""), true); }
//!                             catch { return false; } })();
//! ```
//!
//! Perry is ahead-of-time compiled and can NEVER honor a runtime
//! `new Function(<string built at runtime>)` — that call throws at
//! construction. Historically the empty-body probe const-folded to a working
//! no-op, so the probe reported the capability as *available* — a lie. A
//! feature-detecting JIT (e.g. zod 4's object-validator) then enabled its
//! codegen path and threw on the first real dynamic body, rejecting the
//! surrounding async work.
//!
//! By default the probe now throws at construction (dynamic-codegen reported
//! unavailable), so such libraries take their interpreter fallback. Opt out with
//! `PERRY_EVAL_CSP=0`, which restores the spec-literal empty-function fold. Real
//! literal bodies (`new Function("return 42")`, the `return this` globalThis
//! polyfill) still fold regardless.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Compile `source` (optionally with `PERRY_EVAL_CSP` set) and run it, returning
/// trimmed stdout. Panics on a compile or non-zero run.
fn compile_and_run(dir: &std::path::Path, source: &str, csp_env: Option<&str>) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let mut cmd = Command::new(perry_bin());
    cmd.current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output);
    if let Some(v) = csp_env {
        cmd.env("PERRY_EVAL_CSP", v);
    }
    let compile = cmd.output().expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary exited non-zero: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).trim().to_string()
}

/// The exact zod-4 feature-test shape plus real-literal-body idioms.
const PROBE_SOURCE: &str = r#"
const allowsEval = (() => {
  try { return (new Function(""), true); } catch { return false; }
})();

// A real literal body must still compile+run regardless of the probe result.
const add = new Function("a", "b", "return a + b") as (a: number, b: number) => number;
// The `Function("return this")()` globalThis polyfill must still work.
const g: any = Function("return this")();

process.stdout.write(
  "allowsEval=" + allowsEval +
  " add=" + add(2, 3) +
  " global=" + (typeof g === "object" || typeof g === "undefined" ? "ok" : "NO") + "\n"
);
"#;

#[test]
fn dynamic_function_probe_reports_unavailable_by_default() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = compile_and_run(dir.path(), PROBE_SOURCE, None);
    // Default: the empty-body probe throws -> feature detected as UNAVAILABLE,
    // while real literal bodies + the globalThis polyfill still fold.
    assert_eq!(
        out, "allowsEval=false add=5 global=ok",
        "default probe output"
    );
}

#[test]
fn dynamic_function_probe_opt_out_restores_empty_function() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = compile_and_run(dir.path(), PROBE_SOURCE, Some("0"));
    // Opt out (PERRY_EVAL_CSP=0): the empty-body probe folds to a no-op function,
    // so the feature test reports AVAILABLE (spec-literal Node behavior).
    assert_eq!(
        out, "allowsEval=true add=5 global=ok",
        "opt-out probe output"
    );
}
