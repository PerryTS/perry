//! Regression: the function inliner hoisted a later comma-sequence element's
//! argument-binding `let`s *above* earlier elements' side effects, reordering a
//! read ahead of the store it depends on.
//!
//! Root cause: `inline_calls_in_expr`'s `Expr::Sequence` arm bubbled the inline
//! setup statements (`let <param> = <arg>`) of EVERY element up to before the
//! enclosing statement. A comma-sequence evaluates left-to-right, so element
//! `i>0` runs only after the side effects of elements `0..i`; hoisting its
//! setup above them moves its argument reads ahead of the earlier stores.
//!
//! The triggering shape is an esbuild `__esm` module factory: one big
//! comma-sequence of `Global = ctor({...})` assignments where a later object
//! literal reads an earlier-assigned schema var, e.g.
//! `C = build({ a: Earlier.optional() })`. Inlining `build`/the obj wrapper
//! hoisted `let q = { a: Earlier.optional() }` above the store of `Earlier`,
//! so `Earlier` was still `undefined` → `Cannot read properties of undefined
//! (reading 'optional')`.
//!
//! Fix: only the first sequence element (evaluated before any sibling side
//! effect) may hoist its inline setup; later elements inline only when no setup
//! needs hoisting, otherwise they stay as a plain runtime call.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, entry: &std::path::Path) -> String {
    let output = dir.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(entry)
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
    String::from_utf8_lossy(&run.stdout).to_string()
}

/// Minimal trigger: a comma-sequence of bare assignments to pre-declared vars
/// where a later element does a member access on an earlier-assigned var, and
/// the inlinable callee forces an arg-binding temp. Pre-fix the temp was
/// hoisted above the earlier store → read-before-write.
#[test]
fn later_seq_element_reads_earlier_assigned_var() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
function mk(): any { return { x: 1 }; }
function f(v: any): any { return v; }
var A: any, B: any;
// Bare assignments in a comma-sequence; B's RHS reads A.x via an inlinable
// call whose non-trivial arg forces a `let` arg-binding.
A = mk(), B = f(A.x);
console.log("A=" + JSON.stringify(A));
console.log("B=" + B);
"#,
    )
    .expect("write entry");

    let stdout = compile_and_run(dir.path(), &entry);
    assert!(
        stdout.contains(r#"A={"x":1}"#),
        "A must be assigned before B reads A.x (got: {stdout})"
    );
    assert!(
        stdout.contains("B=1"),
        "B = f(A.x) must read A.x = 1, not undefined (got: {stdout})"
    );
}

/// The esbuild `__esm` schema-factory shape: a lazy-init wrapper around one big
/// comma-sequence of `Global = ctor({...})` assignments, where a later object
/// literal with a computed key reads two earlier-assigned schema vars via
/// `.optional()`. This is the exact reorder that threw
/// `Cannot read properties of undefined (reading 'optional')`.
#[test]
fn esm_schema_factory_computed_key_reads_earlier_vars() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
// esbuild __esm lazy-init memoizer: first call runs the factory once.
var L = (q: any, K?: any) => () => (q && (K = q((q = 0))), K);

function mkSchema(kind: string, props?: any): any {
  return { optional() { return mkSchema("opt"); }, kind, props };
}
function KP(o: any): any { return mkSchema("KP", o); }
function G4(o: any): any { return mkSchema("G4", o); }
function dw(a: any): any { return mkSchema("dw"); }
function O1(): any { return mkSchema("O1"); }

var COMPUTED = "io/related";
// Hoisted module vars assigned inside the factory (esbuild style):
var YE7: any, Wy5: any, C31: any, $h: any;

var factory = L(() => {
  YE7 = dw([O1()]),
  Wy5 = G4({ taskId: O1() }),
  // Computed key + reads of YE7 and Wy5 assigned earlier in THIS sequence.
  C31 = KP({ progressToken: YE7.optional(), [COMPUTED]: Wy5.optional() }),
  $h = G4({ _meta: C31.optional() });
});

factory();
console.log("C31kind=" + C31.kind);
console.log("done");
"#,
    )
    .expect("write entry");

    let stdout = compile_and_run(dir.path(), &entry);
    assert!(
        stdout.contains("C31kind=KP"),
        "C31 must build from YE7/Wy5 stored earlier in the sequence (got: {stdout})"
    );
    assert!(
        stdout.contains("done"),
        "factory must run to completion without a TypeError (got: {stdout})"
    );
}
