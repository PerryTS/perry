//! A CJS default export preserves the name of its function constructor.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn imported_cjs_function_constructor_keeps_its_name() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let library = dir.path().join("lib.cjs");
    let binary = dir.path().join("main_bin");
    std::fs::write(
        &library,
        r#"
function clone() {
  function Ledger(value) { this.value = value; }
  Ledger.prototype = { constructor: Ledger, getValue() { return this.value; } };
  return Ledger;
}
module.exports = clone();
"#,
    )
    .expect("write CJS library");
    std::fs::write(
        &entry,
        r#"
import Ledger from "./lib.cjs";
const value = new Ledger(5);
console.log("NAMES", Ledger.name, value.constructor.name);
console.log("VALUE", value instanceof Ledger, value.getValue());
"#,
    )
    .expect("write entry");

    let compiler = PathBuf::from(env!("CARGO_BIN_EXE_perry"));
    let compile = Command::new(compiler)
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .arg("--no-cache")
        .output()
        .expect("compile fixture");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(binary)
        .current_dir(dir.path())
        .output()
        .expect("run fixture");
    assert!(
        run.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "NAMES Ledger Ledger\nVALUE true 5\n"
    );
}
