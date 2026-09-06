//! End-to-end regression coverage for duplicate class accessors.
//!
//! ECMA-262 ClassDefinitionEvaluation installs class elements in source order,
//! so a later accessor with the same key REPLACES an earlier one. Perry's HIR
//! appended every accessor to `ClassDecl::getters` / `::setters`, and those are
//! consumed with `iter().find(...)` — first match wins — so the SHADOWED
//! definition stayed live and the real one was dropped.
//!
//! The shape below is reduced from Claude-of-Duty's `Spring3`, which pairs an
//! early `set z` (damping) with a later `get z` (displacement). Perry returned
//! the damping coefficient from the shadowed getter, which put a first-person
//! weapon 0.88 m behind the camera, where it clipped and rendered nothing.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn a_later_class_accessor_replaces_an_earlier_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
class Spring3 {
  a = 111;
  c = 222;
  damping = 0;

  // Reading `.z` must reach the LAST getter; writing `.z` must still reach
  // this setter, which no later definition replaces.
  set z(v: number) { this.damping = v; }
  get z(): number { return this.a; }
  get z(): number { return this.c; }
}

// Static and instance accessors are distinct properties and may share a name:
// replacing on the key alone would collapse them.
class Split {
  static _s = "static-first";
  _i = "instance-first";
  static get v(): string { return Split._s; }
  get v(): string { return this._i; }
  static get v(): string { return "static-last"; }
  get v(): string { return "instance-last"; }
}

// A later setter replaces an earlier setter too.
class Sink {
  hits: string[] = [];
  set s(v: string) { this.hits.push("first:" + v); }
  set s(v: string) { this.hits.push("last:" + v); }
}

const spring = new Spring3();
spring.z = 7;
console.log("spring", spring.z, spring.damping);

console.log("split", new Split().v, Split.v);

const sink = new Sink();
sink.s = "x";
console.log("sink", sink.hits.join("|"), sink.hits.length);

// Accessors defined on a class EXPRESSION follow the same rule.
const Expr = class {
  p = 1;
  q = 2;
  get w(): number { return this.p; }
  get w(): number { return this.q; }
};
console.log("expr", new Expr().w);
"#,
    )
    .expect("write fixture");

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

    let run = Command::new(&output).output().expect("run compiled fixture");
    assert!(
        run.status.success(),
        "compiled fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    // Matches `node --experimental-strip-types` on the same source.
    let expected = "spring 222 7\n\
                    split instance-last static-last\n\
                    sink last:x 1\n\
                    expr 2\n";
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected);
}
