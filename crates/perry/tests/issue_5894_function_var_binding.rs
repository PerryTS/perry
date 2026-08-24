//! Regression for #5894 / Test262 `language/reserved-words/unreserved-words.js`.
//!
//! A top-level FunctionDeclaration and a same-named `var` declaration share a
//! single binding. The function value is installed during declaration
//! instantiation; a bare `var f;` is inert, while `var f = value` overwrites it
//! only when execution reaches the initializer. Perry used to pre-register the
//! `var` as a separate undefined local, shadowing the function even before the
//! declaration's source position.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn function_and_var_declarations_share_the_hoisted_binding() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
// The Test262 harness defines `assert` as a function, assigns helper
// properties, and the test later declares `var assert = 1`.
function assert() {}
(assert as any)._isSameValue = "ready";
console.log("helper-before-var:", (assert as any)._isSameValue);
var assert = 1;
console.log("helper-after-var:", assert);

// An uninitialised var redeclaration must not replace the hoisted function.
console.log("bare-before:", bare());
function bare() { return "bare-function"; }
var bare;
console.log("bare-after:", bare());

// A var nested in a block still belongs to the module var scope and shares
// the function binding, even when its initializer never executes.
console.log("nested-before:", nested());
function nested() { return "nested-function"; }
if (false) { var nested = 2; }
console.log("nested-after:", nested());

// Duplicate function declarations install the last declaration at entry;
// the same-named bare var remains inert.
console.log("duplicate-before:", duplicate());
function duplicate() { return "first"; }
function duplicate() { return "second"; }
var duplicate;
console.log("duplicate-after:", duplicate());
console.log("DONE");
"#,
    )
    .expect("write entry");

    let output = dir.path().join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "compiled binary failed\nstdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    for expected in [
        "helper-before-var: ready",
        "helper-after-var: 1",
        "bare-before: bare-function",
        "bare-after: bare-function",
        "nested-before: nested-function",
        "nested-after: nested-function",
        "duplicate-before: second",
        "duplicate-after: second",
        "DONE",
    ] {
        assert!(
            stdout.contains(expected),
            "missing {expected:?}\nstdout:\n{stdout}"
        );
    }
}
