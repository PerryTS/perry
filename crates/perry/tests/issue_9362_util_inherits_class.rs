//! #9362: util.inherits accepts declared classes in either constructor slot.

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

#[test]
fn util_inherits_supports_function_and_class_constructor_pairs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = workspace_root().join("test-files/test_issue_9362_util_inherits_class.ts");
    let output = dir.path().join("main_bin");
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
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "function/function super: function true true false true\n\
function/function chain: true true true function/function\n\
function/class super: function true true false true\n\
function/class chain: true true true function/class\n\
class/function super: function true true false true\n\
class/function chain: true true true class/function\n\
class/class super: function true true false true\n\
class/class chain: true true true class/class\n"
    );
}
