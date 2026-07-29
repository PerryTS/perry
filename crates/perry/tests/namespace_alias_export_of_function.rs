//! Regression test: an export that ALIASES a declared function
//! (`export const alias = impl`) must reach consumers as the function
//! itself, not as a getter-call result.
//!
//! Such an alias lands in BOTH `exported_objects` and `exported_functions`
//! (HIR records the alias with the origin's FuncId). The var-vs-function
//! classification in `run_pipeline` only excluded *declaration* names, so
//! the alias was treated as an exported VARIABLE — whose cross-module
//! symbol convention is a zero-arg getter. But origin-name resolution
//! points `perry_fn_<mod>__<alias>` at the #460 forwarding wrapper (the
//! function BODY), so the "getter" call actually invoked the function:
//! reading `NS.alias` produced the return value of `impl(<zeroed args>)`
//! instead of the closure, and calling it threw "value is not a function".
//!
//! Found compiling the t3 Code server (Effect 4.0.0-beta.78), whose
//! `SchemaParser.ts` is built almost entirely out of this shape
//! (`export const decodeSync = decodeUnknownSync`, `decodeEffect =
//! decodeUnknownEffect`, …). There it surfaced two ways: the consumer-side
//! binding read, and the source module's own namespace populator executing
//! the function while building its namespace object at module init.
//!
//! Expected outputs match `node --experimental-strip-types`.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Writes `inner.ts` + `main.ts` into `dir`, compiles `main.ts`, runs it.
fn compile_and_run(dir: &std::path::Path, inner: &str, main: &str) -> String {
    std::fs::write(dir.join("inner.ts"), inner).expect("write inner");
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, main).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
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
        "compiled binary failed (exit {:?})\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

const INNER: &str = r#"
let ranAtInit = 0;
export function impl(x: number): number {
  ranAtInit += 1;
  return x * 2;
}
export const alias = impl;
export function ranCount(): number {
  return ranAtInit;
}
"#;

/// Namespace import (`import * as NS`): reading `NS.alias` must yield the
/// function without invoking it, and calling it must work. Pre-fix this
/// printed `alias-typeof: number` and then threw "value is not a function".
#[test]
fn namespace_member_alias_of_function_is_the_function() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        INNER,
        r#"
import * as NS from "./inner.ts";
console.log("ran-during-init:", NS.ranCount());
console.log("alias-typeof:", typeof NS.alias);
console.log("alias-call:", NS.alias(21));
"#,
    );
    assert_eq!(
        stdout, "ran-during-init: 0\nalias-typeof: function\nalias-call: 42\n",
        "namespace member aliasing a declared function must read as the function"
    );
}

/// Named import of the same alias — the consumer-side binding read must not
/// execute the origin function either.
#[test]
fn named_import_alias_of_function_is_not_executed_on_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        INNER,
        r#"
import { alias, ranCount } from "./inner.ts";
const held = alias;
console.log("ran-during-init:", ranCount());
console.log("held-typeof:", typeof held);
console.log("held-call:", held(4));
"#,
    );
    assert_eq!(
        stdout, "ran-during-init: 0\nheld-typeof: function\nheld-call: 8\n",
        "reading a named-imported function alias must not invoke it"
    );
}
