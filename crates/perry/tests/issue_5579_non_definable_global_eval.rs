//! Regression (#5579 residual): `eval`/`(0,eval)` must throw a `TypeError`
//! when `CanDeclareGlobalFunction` or `CanDeclareGlobalVar` returns false
//! (ECMA-262 §8.1.1.4.14–15). Three cases:
//!
//! 1. `eval("function NaN(){}")` — `NaN` is non-configurable *and*
//!    non-enumerable on the global object (writable but not enumerable), so
//!    `CanDeclareGlobalFunction("NaN")` → false → TypeError (test262
//!    `language/eval-code/direct/non-definable-global-function`).
//!
//! 2. `eval("function* NaN(){}")` — generator form of the same check (test262
//!    `language/eval-code/direct/non-definable-global-generator`).
//!
//! 3. `eval("var x")` when `Object.preventExtensions(globalThis)` was called —
//!    `CanDeclareGlobalVar("x")` → false when the binding doesn't yet exist and
//!    the global is non-extensible → TypeError (test262
//!    `language/eval-code/direct/non-definable-global-var`).
//!
//! Indirect-eval `(0,eval)(...)` forms of the same tests are covered by the
//! `indirect_` variants below (test262
//! `language/eval-code/indirect/non-definable-global-{function,generator,var}`).
//!
//! The fix: `synth_create_global_fn_binding` now implements the full
//! `CanDeclareGlobalFunction` check; `synth_create_if_absent_stmt` implements
//! `CanDeclareGlobalVar`'s extensibility guard; and `try_indirect_eval_general`
//! now applies `apply_global_eval_hoist` even from nested scopes in
//! global-script mode so the indirect forms fold to the same runtime checks.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Compile `src` in global-script mode and run it. Returns (clean_exit, stdout).
fn run_global(src: &str) -> (bool, String) {
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
        .env("PERRY_ALLOW_EVAL", "1")
        .env("PERRY_GLOBAL_SCRIPT_THIS", "1")
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

// ── Direct eval: CanDeclareGlobalFunction = false ─────────────────────────

/// `eval("function NaN(){}")` must throw `TypeError` at runtime (test262
/// `language/eval-code/direct/non-definable-global-function`).
///
/// `NaN` has descriptor `{writable:true, enumerable:false, configurable:false}`.
/// `CanDeclareGlobalFunction("NaN")` → step 4 (configurable) fails, step 5
/// requires *both* writable AND enumerable — enumerable is false — so step 6
/// returns false → TypeError.
const DIRECT_NON_DEF_FN: &str = r#"
var error;
try {
  eval("function NaN(){}");
} catch (e) {
  error = e;
}
if (!(error instanceof TypeError)) {
  throw new Error("expected TypeError, got: " + error);
}
console.log("PASS");
"#;

#[test]
fn direct_eval_non_definable_global_function_throws() {
    let (ok, out) = run_global(DIRECT_NON_DEF_FN);
    assert!(ok, "binary did not exit cleanly\n{out}");
    assert!(
        out.contains("PASS"),
        "expected TypeError for eval(\"function NaN(){{}}\")\n{out}"
    );
}

/// `eval("function* NaN(){}")` must throw `TypeError` — same `CanDeclareGlobal
/// Function` check, generator form (test262 `non-definable-global-generator`).
const DIRECT_NON_DEF_GEN: &str = r#"
var error;
try {
  eval("function* NaN(){}");
} catch (e) {
  error = e;
}
if (!(error instanceof TypeError)) {
  throw new Error("expected TypeError, got: " + error);
}
console.log("PASS");
"#;

#[test]
fn direct_eval_non_definable_global_generator_throws() {
    let (ok, out) = run_global(DIRECT_NON_DEF_GEN);
    assert!(ok, "binary did not exit cleanly\n{out}");
    assert!(
        out.contains("PASS"),
        "expected TypeError for eval(\"function* NaN(){{}}\")\n{out}"
    );
}

/// `eval("var x")` on a non-extensible global must throw `TypeError`
/// (`CanDeclareGlobalVar` → false when property absent and non-extensible).
/// Test262 `language/eval-code/direct/non-definable-global-var`.
const DIRECT_NON_DEF_VAR: &str = r#"
var nonExtensible;
try {
  Object.preventExtensions(this);
  nonExtensible = !Object.isExtensible(this);
} catch (_) {
  nonExtensible = false;
}

if (nonExtensible) {
  var error;
  try {
    eval("var unlikelyVariableName");
  } catch (e) {
    error = e;
  }
  if (!(error instanceof TypeError)) {
    throw new Error("expected TypeError, got: " + error);
  }
  console.log("PASS");
} else {
  console.log("SKIP");
}
"#;

#[test]
fn direct_eval_non_definable_global_var_throws() {
    let (ok, out) = run_global(DIRECT_NON_DEF_VAR);
    assert!(ok, "binary did not exit cleanly\n{out}");
    assert!(
        out.contains("PASS") || out.contains("SKIP"),
        "expected PASS (TypeError) or SKIP (non-extensible not supported)\n{out}"
    );
}

// ── Indirect eval: same CanDeclareGlobal* checks from a nested scope ──────

/// `(0,eval)("function NaN(){}")` must throw `TypeError` even when called from
/// inside a callback (test262 `language/eval-code/indirect/non-definable-global-
/// function`). Perry's `try_indirect_eval_general` now applies
/// `apply_global_eval_hoist` from nested scopes in global-script mode so the
/// `CanDeclareGlobalFunction` check runs at runtime.
const INDIRECT_NON_DEF_FN: &str = r#"
var threw = false;
(function() {
  try {
    (0, eval)("function NaN(){}");
  } catch (e) {
    threw = e instanceof TypeError;
  }
})();
if (!threw) { throw new Error("expected TypeError"); }
console.log("PASS");
"#;

#[test]
fn indirect_eval_non_definable_global_function_throws() {
    let (ok, out) = run_global(INDIRECT_NON_DEF_FN);
    assert!(ok, "binary did not exit cleanly\n{out}");
    assert!(
        out.contains("PASS"),
        "expected TypeError for (0,eval)(\"function NaN(){{}}\")\n{out}"
    );
}

/// `(0,eval)("function* NaN(){}")` — generator form of the above.
const INDIRECT_NON_DEF_GEN: &str = r#"
var threw = false;
(function() {
  try {
    (0, eval)("function* NaN(){}");
  } catch (e) {
    threw = e instanceof TypeError;
  }
})();
if (!threw) { throw new Error("expected TypeError"); }
console.log("PASS");
"#;

#[test]
fn indirect_eval_non_definable_global_generator_throws() {
    let (ok, out) = run_global(INDIRECT_NON_DEF_GEN);
    assert!(ok, "binary did not exit cleanly\n{out}");
    assert!(
        out.contains("PASS"),
        "expected TypeError for (0,eval)(\"function* NaN(){{}}\")\n{out}"
    );
}

/// Indirect `(0,eval)("var x")` on a non-extensible global (test262
/// `language/eval-code/indirect/non-definable-global-var`).
const INDIRECT_NON_DEF_VAR: &str = r#"
var nonExtensible;
try {
  Object.preventExtensions(this);
  nonExtensible = !Object.isExtensible(this);
} catch (_) {
  nonExtensible = false;
}

if (nonExtensible) {
  var threw = false;
  (function() {
    try {
      (0, eval)("var unlikelyVariableName2");
    } catch (e) {
      threw = e instanceof TypeError;
    }
  })();
  if (!threw) { throw new Error("expected TypeError"); }
  console.log("PASS");
} else {
  console.log("SKIP");
}
"#;

#[test]
fn indirect_eval_non_definable_global_var_throws() {
    let (ok, out) = run_global(INDIRECT_NON_DEF_VAR);
    assert!(ok, "binary did not exit cleanly\n{out}");
    assert!(
        out.contains("PASS") || out.contains("SKIP"),
        "expected PASS (TypeError) or SKIP (non-extensible not supported)\n{out}"
    );
}

// ── Regression guard: definable globals still work ────────────────────────

/// Functions with NEW names (not NaN) are still defined successfully.
const DEFINABLE_GLOBAL_FN: &str = r#"
eval("function __perry_test_fn_ok() { return 42; }");
var result = __perry_test_fn_ok();
if (result !== 42) { throw new Error("expected 42, got: " + result); }
console.log("PASS");
"#;

#[test]
fn definable_global_function_still_works() {
    let (ok, out) = run_global(DEFINABLE_GLOBAL_FN);
    assert!(ok, "binary did not exit cleanly\n{out}");
    assert!(
        out.contains("PASS"),
        "definable function should still work\n{out}"
    );
}
