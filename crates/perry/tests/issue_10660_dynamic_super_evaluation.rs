//! Regression for #10660: `super(...args)` in a chain of repeatedly evaluated
//! class objects must replay the exact parent evaluation rather than reducing
//! it to the shared template id.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn spread_super_uses_the_constructed_class_evaluations_parent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let factory = dir.path().join("factory.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &factory,
        r#"
function attachWithNamespaces(Base: any, namespaces: any) {
  const Commander = class extends Base {
    constructor(...args: any[]) {
      super(...args);
      for (const namespace of Object.keys(namespaces)) {
        this[namespace] = {};
      }
    }
  };
  for (const namespace of Object.keys(namespaces)) {
    Commander.prototype[namespace] = {};
  }
  return Commander;
}

export function attachExtensions(Base: any, namespaces: any) {
  let Commander: any;
  if (namespaces) Commander = attachWithNamespaces(Base, namespaces);
  if (namespaces) Commander = attachWithNamespaces(Commander ?? Base, namespaces);
  return Commander ?? Base;
}
"#,
    )
    .expect("write factory module");
    std::fs::write(
        &entry,
        r#"
import { attachExtensions } from "./factory";

class First {
  kind: string;
  constructor(value: string) { this.kind = "first:" + value; }
}

class Second {
  kind: string;
  constructor(value: string) { this.kind = "second:" + value; }
}

const namespaces = { commands: {} };
// Each call evaluates attachWithNamespaces's class template twice. The outer
// class extends a distinct object with the same template id; the second call
// also overwrites every template-wide registry entry before construction.
const FirstClient = attachExtensions(First, namespaces);
const SecondClient = attachExtensions(Second, namespaces);

console.log(new FirstClient("x").kind);
console.log(new SecondClient("y").kind);
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
        "compiled binary failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "first:x\nsecond:y\n");
}
