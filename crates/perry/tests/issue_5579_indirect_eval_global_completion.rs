//! Regression (#5579, residual): in *global-script* mode a module-top-level
//! indirect `eval` of a constant body — `(0, eval)('<const>')` — must execute
//! as global code and yield the body's completion value, mutating the global
//! bindings it names.
//!
//! Indirect eval runs in the *global* environment, never the caller's lexical
//! scope. Perry models a constant eval body with a scope-capturing completion
//! IIFE (the same mechanism direct eval uses, #1679). For indirect eval that is
//! only sound where the captured enclosing scope already IS the global scope:
//! module top level under `PERRY_GLOBAL_SCRIPT_THIS` (where module-top `var`s
//! and `this` are the global bindings, #5608/#5609). There Perry now folds the
//! body instead of deferring to the runtime global-`eval` thunk (which returned
//! `undefined` for any body that wasn't the `this`/`globalThis` idiom), so the
//! Test262 `language/eval-code/indirect/cptn-nrml-expr-*` and
//! `always-non-strict` completion-value cases resolve against the script-mode
//! Node oracle.
//!
//! Outside that window — a nested scope, or default CJS mode — capturing the
//! enclosing scope would wrongly resolve function/module-locals that real
//! global eval cannot see, so those bodies still defer (this test pins that the
//! fold is gated to module top level + global-script mode).

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Compile `src` and run it. `global_script` toggles `PERRY_GLOBAL_SCRIPT_THIS`.
/// Returns (clean_exit, stdout).
fn compile_and_run(src: &str, global_script: bool) -> (bool, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(&entry, src).expect("write entry");
    let output = dir.path().join("main_bin");

    let mut compile = Command::new(perry_bin());
    compile
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        // Indirect-eval bodies are classified bucket-3 (runtime-unknown) under
        // strict-eval; the Test262 harness runs permissively. Mirror it.
        .env("PERRY_ALLOW_EVAL", "1");
    if global_script {
        compile.env("PERRY_GLOBAL_SCRIPT_THIS", "1");
    } else {
        compile.env_remove("PERRY_GLOBAL_SCRIPT_THIS");
    }
    let compiled = compile.output().expect("run perry compile");
    assert!(
        compiled.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compiled.stdout),
        String::from_utf8_lossy(&compiled.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    (
        run.status.success(),
        String::from_utf8_lossy(&run.stdout).to_string(),
    )
}

/// `language/eval-code/indirect/cptn-nrml-expr-prim.js` shape: the completion
/// value of an indirect eval is its body's expression value, and an assignment
/// inside the body mutates the *global* (module-top) binding it names.
const CPTN_PRIM: &str = r#"
var x;
console.log("assign:", (0, eval)("x = 1"));   // AssignmentExpression -> 1
console.log("num:", (0, eval)("1"));          // NumericLiteral      -> 1
console.log("str:", (0, eval)("'1'"));        // StringLiteral       -> '1'
x = 1;
console.log("update:", (0, eval)("++x"));     // UpdateExpression    -> 2
console.log("x.after:", x);                   // global x mutated    -> 2
console.log("DONE");
"#;

#[test]
fn indirect_eval_completion_value_global_script() {
    let (ok, out) = compile_and_run(CPTN_PRIM, /* global_script */ true);
    assert!(ok, "binary did not exit cleanly\n{out}");
    assert!(
        out.contains("assign: 1"),
        "x = 1 completion must be 1\n{out}"
    );
    assert!(out.contains("num: 1"), "`1` completion must be 1\n{out}");
    assert!(
        out.contains("str: 1"),
        "`'1'` completion must be '1'\n{out}"
    );
    assert!(
        out.contains("update: 2"),
        "`++x` completion must be 2\n{out}"
    );
    assert!(
        out.contains("x.after: 2"),
        "indirect eval must mutate the global `x`\n{out}"
    );
}

/// `cptn-nrml-expr-obj.js` shape: the eval body reads a global object and the
/// completion is that very object (identity preserved).
const CPTN_OBJ: &str = r#"
var x = { tag: "obj" };
var y;
console.log("yx:", (0, eval)("y = x") === x);  // AssignmentExpression -> x
console.log("id:", (0, eval)("x") === x);      // IdentifierReference  -> x
console.log("y:", y === x);                     // global y was set     -> true
console.log("DONE");
"#;

#[test]
fn indirect_eval_object_identity_global_script() {
    let (ok, out) = compile_and_run(CPTN_OBJ, /* global_script */ true);
    assert!(ok, "binary did not exit cleanly\n{out}");
    assert!(
        out.contains("yx: true"),
        "`y = x` completion must be x\n{out}"
    );
    assert!(out.contains("id: true"), "`x` completion must be x\n{out}");
    assert!(
        out.contains("y: true"),
        "global `y` must be assigned x\n{out}"
    );
}

/// Indirect eval is *always sloppy* (no inherited strictness), so a body that
/// only a sloppy context admits — `with`, an undeclared assignment — runs and
/// its side effects land. (`language/eval-code/indirect/always-non-strict.js`
/// minus the strict-reserved-word `var static` line, a separate gap.)
const NON_STRICT: &str = r#"
var count = 0;
(0, eval)('with ({}) {} count += 1;');
(0, eval)('unresolvable = null; count += 1;');
console.log("count:", count);  // -> 2
console.log("DONE");
"#;

#[test]
fn indirect_eval_is_always_sloppy_global_script() {
    let (ok, out) = compile_and_run(NON_STRICT, /* global_script */ true);
    assert!(ok, "binary did not exit cleanly\n{out}");
    assert!(
        out.contains("count: 2"),
        "both sloppy bodies must run\n{out}"
    );
}

/// The fold is gated to module top level: an indirect eval inside a function
/// must NOT capture that function's locals (real global eval cannot see them).
/// Here the inner `local` is invisible to the indirect eval, so `typeof local`
/// resolves as `"undefined"` in global scope and the program exits cleanly.
/// (Pins that the completion IIFE is not applied at nested scope, where it would
/// wrongly resolve `local` to its `number` value.)
const NESTED_NOT_FOLDED: &str = r#"
function f() {
  var local = 7;
  return (0, eval)("typeof local");  // global eval can't see `local`
}
console.log("typeof:", f());  // -> "undefined", never "number"
console.log("DONE");
"#;

#[test]
fn indirect_eval_nested_does_not_capture_locals() {
    let (ok, out) = compile_and_run(NESTED_NOT_FOLDED, /* global_script */ true);
    assert!(ok, "binary did not exit cleanly\n{out}");
    // Positive expectation: the body resolves against the global env (where
    // `local` does not exist), so `typeof local` is `"undefined"` — never the
    // function local's `number` value.
    assert!(
        out.contains("typeof: undefined") && out.contains("DONE"),
        "indirect eval should resolve `typeof local` as \"undefined\" in global scope\n{out}"
    );
    assert!(
        !out.contains("typeof: number"),
        "indirect eval must not capture the enclosing function's `local`\n{out}"
    );
}

/// Regression (#5735, cluster 2): the completion value of a statement list whose
/// last evaluated statement is a *declaration* must fall through to the prior
/// statement — a declaration produces an *empty* completion (test262
/// `language/statements/{function,async-function,generators,variable}/cptn-*`,
/// `language/statementList/eval-fn-block`).
///
/// In global-script mode a top-level `function` declaration inside the eval body
/// is published to the global environment via CreateGlobalFunctionBinding, whose
/// `Object.defineProperty(globalThis, …)` call *returns `globalThis`*. The
/// completion tracker rewrote that publish into `__perry_cv = Object.define…`,
/// so a declaration-only body (`eval("function f() {}")`) wrongly yielded the
/// global object instead of `undefined`. The publish is now `void`-wrapped (it
/// is declaration-instantiation machinery, not a statement of the source), so it
/// keeps an empty completion. A preceding value statement still shows through
/// (`eval("1; function f() {}")` === 1).
const CPTN_DECL: &str = r#"
console.log("fn:", eval("function f() {}"));                 // -> undefined
console.log("fn1:", eval("1; function f1() {}"));            // -> 1
console.log("gen:", eval("function* g() {}"));               // -> undefined
console.log("gen1:", eval("1; function* g1() {}"));          // -> 1
console.log("async:", eval("async function af() {}"));       // -> undefined
console.log("var:", eval("var v1;"));                        // -> undefined
console.log("varinit:", eval("var v2 = 2;"));                // -> undefined
console.log("var7:", eval("7; var v8;"));                    // -> 7
console.log("var9:", eval("9; var v10 = 10;"));              // -> 9 (init falls through)
console.log("var11:", eval("11; var v12 = 12, v13;"));       // -> 11
console.log("var14:", eval("14; var v15, v16 = 16;"));       // -> 14
console.log("fnblock:", eval("function fn() {}{}"));         // -> undefined
console.log("DONE");
"#;

#[test]
fn eval_declaration_completion_is_empty_global_script() {
    let (ok, out) = compile_and_run(CPTN_DECL, /* global_script */ true);
    assert!(ok, "binary did not exit cleanly\n{out}");
    assert!(
        out.contains("fn: undefined"),
        "`function f(){{}}` -> undefined\n{out}"
    );
    assert!(out.contains("fn1: 1"), "`1; function f1(){{}}` -> 1\n{out}");
    assert!(
        out.contains("gen: undefined"),
        "`function* g(){{}}` -> undefined\n{out}"
    );
    assert!(
        out.contains("gen1: 1"),
        "`1; function* g1(){{}}` -> 1\n{out}"
    );
    assert!(
        out.contains("async: undefined"),
        "`async function af(){{}}` -> undefined\n{out}"
    );
    assert!(
        out.contains("var: undefined"),
        "`var v1;` -> undefined\n{out}"
    );
    assert!(
        out.contains("varinit: undefined"),
        "`var v2 = 2;` -> undefined\n{out}"
    );
    assert!(out.contains("var7: 7"), "`7; var v8;` -> 7\n{out}");
    // The empty completion of a `var` declaration (even with an initializer)
    // falls through to the preceding statement's value, not `undefined`.
    assert!(out.contains("var9: 9"), "`9; var v10 = 10;` -> 9\n{out}");
    assert!(
        out.contains("var11: 11"),
        "`11; var v12 = 12, v13;` -> 11\n{out}"
    );
    assert!(
        out.contains("var14: 14"),
        "`14; var v15, v16 = 16;` -> 14\n{out}"
    );
    assert!(
        out.contains("fnblock: undefined"),
        "`function fn(){{}}{{}}` -> undefined\n{out}"
    );
}

/// Regression (#5592): `(0, eval)('arguments;')` inside a class field
/// initializer (at module top level) must see the global `arguments` binding,
/// not produce `undefined`. The fold was blocked by a `current_class.is_none()`
/// guard in `eval_is_module_top_global`; class field initializers don't create a
/// new variable environment so the guard was overly conservative.
const CLASS_FIELD_INDIRECT_EVAL_ARGUMENTS: &str = r#"
var arguments = 1;
class C {
  x = (0, eval)('arguments;');
}
var result = new C().x;
console.log("result:", result);
console.log("DONE");
"#;

#[test]
fn indirect_eval_in_class_field_sees_global_arguments() {
    let (ok, out) = compile_and_run(
        CLASS_FIELD_INDIRECT_EVAL_ARGUMENTS,
        /* global_script */ true,
    );
    assert!(ok, "binary did not exit cleanly\n{out}");
    assert!(
        out.contains("result: 1"),
        "indirect eval in class field must see global `arguments = 1`\n{out}"
    );
}

/// Same as above but with a private class field (#5592, private variant).
const PRIVATE_CLASS_FIELD_INDIRECT_EVAL_ARGUMENTS: &str = r#"
var arguments = 1;
class C {
  #x = (0, eval)('arguments;');
  getX() { return this.#x; }
}
var result = new C().getX();
console.log("result:", result);
console.log("DONE");
"#;

#[test]
fn indirect_eval_in_private_class_field_sees_global_arguments() {
    let (ok, out) = compile_and_run(
        PRIVATE_CLASS_FIELD_INDIRECT_EVAL_ARGUMENTS,
        /* global_script */ true,
    );
    assert!(ok, "binary did not exit cleanly\n{out}");
    assert!(
        out.contains("result: 1"),
        "indirect eval in private class field must see global `arguments = 1`\n{out}"
    );
}
