//! Regression coverage for #8406's dynamic-dispatch shape shortcut.
//!
//! An exact canonical class shape may bypass the runtime own-property scan,
//! but only when that canonical layout cannot itself contain the method name.
//! A declared function field and a later own-property assignment must both
//! continue to override an inherited prototype method.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn canonical_and_mutated_own_method_overrides_survive_shape_shortcut() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
interface Runner { run(): string }

class Base {
  run(): string { return "base"; }
}

class FieldOverride extends Base {
  run = (): string => "field";
}

class MutatedOverride extends Base {}

function invoke(value: Runner): string {
  return value.run();
}

const mutated: any = new MutatedOverride();
mutated.run = (): string => "mutated";

console.log(invoke(new Base()), invoke(new FieldOverride()), invoke(mutated));
"#,
    )
    .expect("write source");

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
    assert_eq!(String::from_utf8_lossy(&run.stdout), "base field mutated\n");
}
