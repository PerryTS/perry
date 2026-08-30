//! Regression for Effect Schema's `isSchema` predicate. Effect brands the
//! class object returned by a factory with a computed string static, then asks
//! whether that key exists on a declared subclass through generic `key in u`.
//! Static reads already inherited the value; generic `in` incorrectly reported
//! false because the ClassRef path only checked own dynamic data fields.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn generic_in_finds_computed_string_static_on_class_expression_parent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
const TypeId = "~effect/Schema/Schema";

function makeClass() {
  return class {
    static readonly [TypeId] = TypeId;
    static readonly presentButUndefined = undefined;
  };
}

const Base = makeClass();
class Derived extends Base {}

function hasProperty(value: unknown, key: PropertyKey) {
  return (typeof value === "object" && value !== null || typeof value === "function")
    && key in value;
}

console.log(hasProperty(Derived, TypeId));
console.log((Derived as any)[TypeId] === TypeId);
console.log(hasProperty(Derived, "presentButUndefined"));
"#,
    )
    .expect("write entry");

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
    assert_eq!(String::from_utf8_lossy(&run.stdout), "true\ntrue\ntrue\n");
}
