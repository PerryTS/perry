//! Regression test for #5130: a nested `namespace` was not emitted — accessing
//! a member of an inner namespace yielded `undefined` (and then threw on the
//! property access). Top-level namespaces worked.
//!
//! Root cause: `lower_namespace_as_class` dropped nested `TsModule` items (the
//! `ExportDecl`/`Stmt` match arms fell through to `_ => {}`), and the dotted
//! `TsNamespaceDecl` body returned an empty class.
//!
//! Fix: nested namespaces are lowered recursively as their own synthetic class
//! registered under a qualified `Outer.Inner` name, and the outer namespace
//! gains a static field `Inner` holding a `ClassRef` to it. `Outer.Inner` then
//! resolves to the inner namespace and `Outer.Inner.member` reads its statics
//! (a runtime property/method access on a class-ref resolves static
//! fields/methods). Works to any nesting depth.

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
        "compiled binary failed (pre-fix: nested namespace undefined)\nstatus: {:?}\n\
         stdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn nested_namespace_members_resolve() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
namespace G {
  export const PI = 3.14;
  export function area(r: number) { return PI * r * r; }
  export namespace Nested {
    export const value = 42;
    export function f() { return value + 1; }
  }
}
console.log(G.area(2));
console.log(G.Nested.value);
console.log(G.Nested.f());
"#,
    );
    assert_eq!(stdout, "12.56\n42\n43\n");
}

#[test]
fn deeply_nested_namespaces_and_cross_level_refs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
namespace Outer {
  export const base = 100;
  export namespace Mid {
    export const m = 10;
    export function getM() { return m; }
    export namespace Inner {
      export const deep = 1;
      export function sum() { return deep + m + base; } // reads all enclosing scopes
    }
  }
}
console.log(Outer.Mid.m);
console.log(Outer.Mid.getM());
console.log(Outer.Mid.Inner.deep);
console.log(Outer.Mid.Inner.sum());
const M = Outer.Mid;            // aliasing a nested namespace to a value
console.log(M.m, M.Inner.deep);
"#,
    );
    assert_eq!(stdout, "10\n10\n1\n111\n10 1\n");
}
