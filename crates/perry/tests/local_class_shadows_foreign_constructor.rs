//! Regression: broad cross-module class metadata must not override a local
//! class with the same name in the constructor-call table.
//!
//! A namespace import exposes every exported class as dispatch metadata. The
//! imported class below and the entry module's base are both named `Node`.
//! Before the fix, the synthesized constructor for `Leaf extends Node` called
//! the foreign `Node(internal, { id })` with no arguments and threw while
//! destructuring the missing second argument.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn local_base_wins_over_same_named_namespace_import_class() {
    let dir = tempfile::tempdir().expect("tempdir");
    let foreign = dir.path().join("foreign.mjs");
    let entry = dir.path().join("main.mjs");
    let output = dir.path().join("main_bin");

    std::fs::write(
        &foreign,
        r#"
export class Node {
  constructor(internal, { id }) {
    this.internal = internal;
    this.id = id;
  }
}
"#,
    )
    .expect("write foreign module");
    std::fs::write(
        &entry,
        r#"
import * as foreign from "./foreign.mjs";
class Node {}
class Leaf extends Node {}
const leaf = new Leaf();
console.log(leaf instanceof Node, typeof foreign.Node);
"#,
    )
    .expect("write entry module");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("compile fixture");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir.path())
        .output()
        .expect("run fixture");
    assert!(
        run.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "true function");
}
