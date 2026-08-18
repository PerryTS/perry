//! Regression (#5841): sloppy-eval-created global bindings must publish with
//! `configurable: true` descriptors.
//!
//! `EvalDeclarationInstantiation` (Annex B.3.3.3) always calls
//! `CreateGlobalVarBinding(vn, /* D = */ true)` and `CreateGlobalFunctionBinding
//! (F, V, /* D = */ true)` — unlike a top-level Script's own
//! `GlobalDeclarationInstantiation`, which passes `D = false`. Perry's
//! `synth_create_if_absent_stmt` (the `CreateGlobalVarBinding` create-if-absent
//! prelude shared by top-level `var` declarations and nested block/`if`/`switch`
//! function declarations) hardcoded `configurable: false`, so every global
//! binding freshly created by an `eval` — a top-level `var`, or a legacy-hoisted
//! block function — published as non-configurable, failing test262's
//! `verifyProperty(..., { configurable: true })` checks (test262
//! `annexB/language/eval-code/{direct,indirect}/global-*-eval-global-init`,
//! `language/eval-code/{direct,indirect}/var-env-var-init-global-new`).
//!
//! A *pre-existing* global (declared by the enclosing script, not by the eval
//! itself) is untouched by the create-if-absent prelude — only its value is
//! reassigned — so its original descriptor (including a `false` configurable
//! from the script's own `GlobalDeclarationInstantiation`) must survive
//! (test262 `var-env-var-init-global-exstng`).

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
    let mut output = String::from_utf8_lossy(&run.stdout).to_string();
    output.push_str(&String::from_utf8_lossy(&run.stderr));
    (run.status.success(), output)
}

/// A block-scoped function declaration hoisted by a *global* direct `eval` must
/// publish a `configurable: true` global binding (test262 `annexB/language/
/// eval-code/direct/global-block-decl-eval-global-init`).
const DIRECT_BLOCK_FN_CONFIGURABLE: &str = r#"
eval("{ function f() {} }");
var d = Object.getOwnPropertyDescriptor(globalThis, "f");
if (d.configurable !== true) {
  throw new Error("expected f.configurable === true, got " + d.configurable);
}
console.log("PASS");
"#;

#[test]
fn direct_eval_block_function_binding_is_configurable() {
    let (ok, out) = run_global(DIRECT_BLOCK_FN_CONFIGURABLE);
    assert!(ok, "binary did not exit cleanly\n{out}");
    assert!(out.contains("PASS"), "{out}");
}

/// A top-level `var` declared inside a *global* direct `eval`, for a name with
/// no pre-existing global, must publish a `configurable: true` binding
/// (test262 `language/eval-code/direct/var-env-var-init-global-new`).
const DIRECT_NEW_VAR_CONFIGURABLE: &str = r#"
var initial;
eval("initial = __perry_5841_new_var; var __perry_5841_new_var;");
var d = Object.getOwnPropertyDescriptor(globalThis, "__perry_5841_new_var");
if (d.configurable !== true) {
  throw new Error("expected configurable === true, got " + d.configurable);
}
if (d.value !== undefined || d.writable !== true || d.enumerable !== true) {
  throw new Error("unexpected descriptor: " + JSON.stringify(d));
}
if (initial !== undefined) {
  throw new Error("expected pre-declaration read to be undefined, got " + initial);
}
console.log("PASS");
"#;

#[test]
fn direct_eval_new_global_var_binding_is_configurable() {
    let (ok, out) = run_global(DIRECT_NEW_VAR_CONFIGURABLE);
    assert!(ok, "binary did not exit cleanly\n{out}");
    assert!(out.contains("PASS"), "{out}");
}

/// A direct-eval `var` that reuses a Script-level global must update its value
/// without changing the Script binding's non-configurable descriptor (test262
/// `language/eval-code/direct/var-env-var-init-global-exstng`, #5903).
const DIRECT_EXISTING_VAR_VALUE_UPDATED: &str = r#"
var initial;
var __perry_5841_existing = 23;
eval("initial = __perry_5841_existing; var __perry_5841_existing = 45;");
if (initial !== 23) {
  throw new Error("expected initial value 23, got " + initial);
}
if (__perry_5841_existing !== 45) {
  throw new Error("expected value 45, got " + __perry_5841_existing);
}
var d = Object.getOwnPropertyDescriptor(globalThis, "__perry_5841_existing");
if (d.value !== 45 || d.writable !== true || d.enumerable !== true || d.configurable !== false) {
  throw new Error("unexpected descriptor: " + JSON.stringify(d));
}
console.log("PASS");
"#;

#[test]
fn direct_eval_existing_global_var_value_is_updated() {
    let (ok, out) = run_global(DIRECT_EXISTING_VAR_VALUE_UPDATED);
    assert!(ok, "binary did not exit cleanly\n{out}");
    assert!(out.contains("PASS"), "{out}");
}

/// Indirect-eval form of [`direct_eval_block_function_binding_is_configurable`]
/// (test262 `annexB/language/eval-code/indirect/global-block-decl-eval-global-init`).
const INDIRECT_BLOCK_FN_CONFIGURABLE: &str = r#"
(0, eval)("{ function g() {} }");
var d = Object.getOwnPropertyDescriptor(globalThis, "g");
if (d.configurable !== true) {
  throw new Error("expected g.configurable === true, got " + d.configurable);
}
console.log("PASS");
"#;

#[test]
fn indirect_eval_block_function_binding_is_configurable() {
    let (ok, out) = run_global(INDIRECT_BLOCK_FN_CONFIGURABLE);
    assert!(ok, "binary did not exit cleanly\n{out}");
    assert!(out.contains("PASS"), "{out}");
}

/// Indirect-eval form of [`direct_eval_new_global_var_binding_is_configurable`]
/// (test262 `language/eval-code/indirect/var-env-var-init-global-new`).
const INDIRECT_NEW_VAR_CONFIGURABLE: &str = r#"
var initial;
(0, eval)("initial = __perry_5841_new_var2; var __perry_5841_new_var2;");
var d = Object.getOwnPropertyDescriptor(globalThis, "__perry_5841_new_var2");
if (d.configurable !== true) {
  throw new Error("expected configurable === true, got " + d.configurable);
}
if (initial !== undefined) {
  throw new Error("expected pre-declaration read to be undefined, got " + initial);
}
console.log("PASS");
"#;

#[test]
fn indirect_eval_new_global_var_binding_is_configurable() {
    let (ok, out) = run_global(INDIRECT_NEW_VAR_CONFIGURABLE);
    assert!(ok, "binary did not exit cleanly\n{out}");
    assert!(out.contains("PASS"), "{out}");
}

/// Indirect-eval form of [`direct_eval_existing_global_var_value_is_updated`]
/// (test262 `language/eval-code/indirect/var-env-var-init-global-exstng`).
const INDIRECT_EXISTING_VAR_VALUE_UPDATED: &str = r#"
var initial;
var __perry_5903_existing = 23;
(0, eval)("initial = __perry_5903_existing; var __perry_5903_existing = 45;");
if (initial !== 23) {
  throw new Error("expected initial value 23, got " + initial);
}
if (__perry_5903_existing !== 45) {
  throw new Error("expected value 45, got " + __perry_5903_existing);
}
var d = Object.getOwnPropertyDescriptor(globalThis, "__perry_5903_existing");
if (d.value !== 45 || d.writable !== true || d.enumerable !== true || d.configurable !== false) {
  throw new Error("unexpected descriptor: " + JSON.stringify(d));
}
console.log("PASS");
"#;

#[test]
fn indirect_eval_existing_global_var_value_and_descriptor_are_updated() {
    let (ok, out) = run_global(INDIRECT_EXISTING_VAR_VALUE_UPDATED);
    assert!(ok, "binary did not exit cleanly\n{out}");
    assert!(out.contains("PASS"), "{out}");
}
