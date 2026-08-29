//! Regression coverage for #9053: aggregate scalar replacement must not
//! delete a carrier whose binding is consumed through another module.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn exported_array_of_objects_remains_materialized() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("values.js"),
        r#"
export const VALUES = [
  { x: 1, label: "one" },
  { x: 2, label: "two" },
];
"#,
    )
    .expect("write producer module");
    std::fs::write(
        dir.path().join("main.ts"),
        r#"
import { VALUES } from "./values.js";

console.log("length:", VALUES.length);
console.log("identity:", VALUES === VALUES);
for (const value of VALUES) {
  console.log(value.x, value.label);
}
"#,
    )
    .expect("write entry module");

    let output = dir.path().join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg("main.ts")
        .arg("--no-cache")
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
        .current_dir(dir.path())
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "length: 2\nidentity: true\n1 one\n2 two\n"
    );
}
