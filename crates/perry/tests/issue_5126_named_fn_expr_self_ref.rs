//! Regression test for #5126: a named function expression could not
//! reference its own name from inside its body — a recursive self-call threw
//! `ReferenceError: f is not defined`.
//!
//! Per spec, the identifier of a `NamedFunctionExpression` is bound (read-only)
//! within the function's own body, independent of the binding it is later
//! assigned to:
//!
//! ```ts
//! const fact = function f(n) { return n <= 1 ? 1 : n * f(n - 1); };
//! fact(5); // 120 — `f` resolves to the function itself
//! ```
//!
//! Fix: when a named function expression's body actually captures its own
//! name, lower it through an immediately-invoked arrow that binds the name to
//! the function value (`(() => { let f = <fn>; return f; })()`). That `let f =
//! <closure referencing f>` shape is exactly the self-recursive-`const`
//! pattern codegen already boxes, so `f` resolves through its heap box. A
//! named function expression that does NOT reference its own name keeps its
//! bare closure (and its `.name`).

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
        "compiled binary failed (pre-fix: 'ReferenceError: f is not defined')\n\
         status: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// The canonical repro from the issue plus a second self-recursive case and a
/// non-self-referential named function expression (whose `.name` must survive).
#[test]
fn named_fn_expr_can_reference_its_own_name() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const fact = function f(n: number): number { return n <= 1 ? 1 : n * f(n - 1); };
console.log(fact(5));
const fib = function fb(n: number): number { return n < 2 ? n : fb(n - 1) + fb(n - 2); };
console.log(fib(10));
const dbl = function named(x: number) { return x * 2; };
console.log(dbl(21), dbl.name);
"#,
    );
    assert_eq!(stdout, "120\n55\n42 named\n");
}

/// The self-binding must shadow an outer binding of the same name, capture
/// enclosing-scope variables, and work for generators and IIFEs.
#[test]
fn named_fn_expr_self_ref_edge_cases() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const mul = 3;
const step = function step(n: number): number { return n <= 0 ? 0 : mul + step(n - 1); };
console.log(step(4));
const gen = function* g(n: number): Generator<number> { if (n > 0) { yield n; yield* g(n - 1); } };
console.log([...gen(3)].join(","));
console.log((function fac(n: number): number { return n <= 1 ? 1 : n * fac(n - 1); })(6));
"#,
    );
    assert_eq!(stdout, "12\n3,2,1\n720\n");
}
