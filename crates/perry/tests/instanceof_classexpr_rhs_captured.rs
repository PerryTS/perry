//! Regression: a class-expression binding used *only* as the RHS of
//! `instanceof` inside a function evaluated to `undefined`.
//!
//! `const C = class {}` is a module-level local. When the sole use of `C` is
//! as an `instanceof` right-hand side inside a nested function
//! (`x instanceof C`), the codegen "referenced locals" collector
//! (collectors/refs.rs, and its siblings — i32-locals, escape, closure
//! capture, monomorph) walked only the InstanceOf `expr` (LHS) and skipped
//! `ty_expr` (the RHS class value). So `C` looked unreferenced, its
//! initializing store was eliminated, and `LocalGet` read an uninitialized
//! slot → the runtime received `undefined` as the RHS and threw
//! `TypeError: Right-hand side of 'instanceof' is not an object`.
//!
//! Node evaluates the RHS normally, so `x instanceof C` returns a boolean.
//! Any *other* use of `C` (`const x = C; x instanceof …`, `f(C)`) masked the
//! bug by making the collector see `C`. Fix: every InstanceOf analysis pass
//! visits `ty_expr`.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
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
    let run = Command::new(&output)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed (pre-fix: 'Right-hand side of instanceof is not an object')\n\
         status: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// The minimal case: a plain class-expression const used only as an
/// instanceof RHS inside a plain function.
#[test]
fn classexpr_instanceof_rhs_in_function() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const C: any = class {};
function fn() { console.log(({}) instanceof C, (new C()) instanceof C); }
fn();
"#,
    );
    assert_eq!(stdout, "false true\n");
}

/// Multiple class-expression consts, one with `static [Symbol.hasInstance]`,
/// each used only as an instanceof RHS inside a function (the shape that
/// first surfaced this: a class-expression `H` whose `hasInstance` brand
/// check ran inside an async body).
#[test]
fn classexpr_instanceof_rhs_multiple_and_hasinstance() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const H: any = class { static [Symbol.hasInstance](v: any) { return v === 7; } };
const P1: any = class {};
const P2: any = class {};
function sync() {
  console.log("H", 7 instanceof H, 8 instanceof H);
  console.log("P1", ({}) instanceof P1, "P2", ({}) instanceof P2);
}
sync();
(async () => {
  console.log("async-H", 7 instanceof H);
  console.log("async-P1", ({}) instanceof P1);
})();
"#,
    );
    assert_eq!(
        stdout,
        "H true false\nP1 false P2 false\nasync-H true\nasync-P1 false\n"
    );
}

/// The RHS class expression captured into an actual closure (arrow), not just
/// a top-level function — exercises the closure-capture collector arm.
#[test]
fn classexpr_instanceof_rhs_in_closure() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const Klass: any = class {};
const check = (v: any) => v instanceof Klass;
console.log(check({}), check(new Klass()));
[1, 2].forEach(() => { if (({}) instanceof Klass) console.log("unreachable"); });
console.log("done");
"#,
    );
    assert_eq!(stdout, "false true\ndone\n");
}
