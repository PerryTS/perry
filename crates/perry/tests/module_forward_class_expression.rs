//! Regression coverage for forward references between module-level class
//! expression bindings.  Dependency builds commonly contain this emitted ESM
//! shape (`var A = class { ... new B() ... }; var B = class { ... }`).

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn method_resolves_later_module_class_expression_binding() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(
        root.join("main.ts"),
        r#"
var Builder = class {
  build() {
    return new Later(42);
  }
};

var Later = class {
  value: number;
  constructor(value: number) {
    this.value = value;
  }
};

console.log("value=" + new Builder().build().value);
"#,
    )
    .expect("write source");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(root.join("main.ts"))
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

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "value=42\n");
}

#[test]
fn implicit_derived_class_expression_initializes_parent_before_own_fields() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(
        root.join("main.ts"),
        r#"
var Base = class {
  config: { mode: string };
  constructor(_table: unknown, config: { mode: string }) {
    this.config = config;
  }
};

var Middle = class extends Base {};

var Child = class extends Middle {
  mode = this.config.mode;
};

console.log("mode=" + new Child({}, { mode: "boolean" }).mode);
"#,
    )
    .expect("write source");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(root.join("main.ts"))
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

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "mode=boolean\n");
}
