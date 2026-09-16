//! #10353: the builder fold may skip statements between a `{}` binding and
//! its stores, which sinks the ALLOCATION below them. These are the observable
//! consequences — the HIR tests in `perry-hir/tests/builder_fold_gap.rs` pin
//! which shapes fold, this file pins that folding them changes nothing a
//! program can see.

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
        .current_dir(dir.path())
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

#[test]
fn a_skipped_constant_binding_leaves_the_object_unchanged() {
    // The issue's repro plus the shape observations a fold could disturb:
    // key order, own-property-ness and `in` must all match the unfolded form.
    let stdout = compile_and_run(
        r#"
function build(): any {
  const o: any = {};
  const x = 1;
  o.p1 = x; o.p2 = x + 1; o.p3 = x + 2;
  return o;
}
const o = build();
console.log(JSON.stringify(o));
console.log(Object.keys(o).join(","));
console.log("p1" in o, "p9" in o, Object.prototype.hasOwnProperty.call(o, "p3"));
"#,
    );
    assert_eq!(
        stdout,
        "{\"p1\":1,\"p2\":2,\"p3\":3}\np1,p2,p3\ntrue false true\n"
    );
}

#[test]
fn skipped_bindings_still_run_before_the_literal() {
    let stdout = compile_and_run(
        r#"
const o: any = {};
const a = 2;
const b = a * 3;
o.sum = a + b;
o.b = b;
console.log(JSON.stringify(o));
"#,
    );
    assert_eq!(stdout, "{\"sum\":8,\"b\":6}\n");
}

#[test]
fn a_gap_statement_that_reads_the_binding_still_sees_the_object() {
    // `peek()` does not name `o` at the call site, but it reads it. Sinking
    // the allocation below the call would make this a TDZ ReferenceError.
    let stdout = compile_and_run(
        r#"
function peek(): any { return o; }
const o: any = {};
const seen = peek();
o.p1 = 1;
console.log(seen === o, seen.p1, JSON.stringify(o));
"#,
    );
    assert_eq!(stdout, "true 1 {\"p1\":1}\n");
}

#[test]
fn an_alias_taken_in_the_gap_still_observes_the_stores() {
    let stdout = compile_and_run(
        r#"
const o: any = {};
const alias = o;
o.p1 = 1;
o.p2 = 2;
console.log(alias === o, JSON.stringify(alias));
"#,
    );
    assert_eq!(stdout, "true {\"p1\":1,\"p2\":2}\n");
}

#[test]
fn a_populated_literal_keeps_its_own_evaluation_order() {
    let stdout = compile_and_run(
        r#"
function run(): string {
  try {
    const o: any = { a: (y as any) };
    const y = 1;
    o.b = 2;
    return JSON.stringify(o);
  } catch (e: any) {
    return "threw " + e.name;
  }
}
console.log(run());
"#,
    );
    assert_eq!(stdout, "threw ReferenceError\n");
}
