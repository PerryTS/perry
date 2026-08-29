//! Regression coverage for #9051: auto-optimization must retain the reified
//! Math namespace when a method is read as a first-class function.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
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
        // This regression exists only in the default feature-pruned runtime.
        .env_remove("PERRY_NO_AUTO_OPTIMIZE")
        .env("PERRY_WORKSPACE_ROOT", workspace_root())
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
        "compiled binary failed (pre-fix: extracted Math method was undefined)\n\
         status: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn extracted_math_methods_survive_auto_optimization() {
    // Class bodies live outside module init/functions in final HIR. Keep this
    // case first so a detector that forgets `hir_module.classes` cannot pass
    // because another compile already warmed a global-math archive.
    let class_method = compile_and_run(
        r#"
class Trig {
  cosAtZero() {
    const cos = Math.cos;
    return cos(0);
  }
}

console.log(new Trig().cosAtZero());
console.log(Math.cos(0));
"#,
    );
    assert_eq!(class_method, "1\n1\n");

    let top_level = compile_and_run(
        r#"
const cos = Math.cos;
console.log(cos(0));
console.log(Math.cos(0));
"#,
    );
    assert_eq!(top_level, "1\n1\n");
}
