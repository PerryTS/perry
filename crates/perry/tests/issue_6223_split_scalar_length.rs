//! Regression tests for #6223 scalar replacement of string-split parts whose
//! only observable property is `.length`.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-auto-optimize")
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
        "compiled binary failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).to_string()
}

#[test]
fn split_part_length_avoids_part_materialization_with_utf16_length() {
    let stdout = compile_and_run(
        r#"
function partLength(value: string): number {
  const parts = value.split(",");
  return parts[1].length;
}
console.log(partLength("a,bc,d"));
console.log(partLength("a,😀,d"));
"#,
    );
    assert_eq!(stdout, "2\n2\n");
}

#[test]
fn split_part_value_stays_materialized_when_it_has_a_non_length_use() {
    let stdout = compile_and_run(
        r#"
function readPart(value: string): string {
  const parts = value.split("-");
  console.log(parts[1].length);
  return parts[1];
}
console.log(readPart("a-bc"));
"#,
    );
    assert_eq!(stdout, "2\nbc\n");
}

#[test]
fn non_escaping_uppercase_pipeline_keeps_utf16_and_source_snapshot() {
    let stdout = compile_and_run(
        r#"
function pipeline(value: string): string {
  const upper = value.toUpperCase();
  const parts = upper.split("-");
  return parts[1].length + ":" + upper.indexOf("SS");
}
let source = "a-9";
const upper = source.toUpperCase();
source = "changed";
const parts = upper.split("-");
console.log(parts[1].length + ":" + upper.indexOf("9"));
console.log(pipeline("straße-😀"));
"#,
    );
    assert_eq!(stdout, "1:2\n2:4\n");
}

#[test]
fn uppercase_result_materializes_when_an_unsupported_use_is_observed() {
    let stdout = compile_and_run(
        r#"
function materialize(value: string): string {
  const upper = value.toUpperCase();
  return upper;
}
console.log(materialize("a-bc"));
"#,
    );
    assert_eq!(stdout, "A-BC\n");
}
