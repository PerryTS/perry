//! #9148: function-object expandos must enumerate in property-creation order.
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
fn function_expandos_follow_ordinary_own_property_key_order() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = workspace_root().join("test-files/test_issue_9148_function_expando_order.ts");
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
        "[\"tag\",\"other\"]\n\
[\"2\",\"10\",\"tag\",\"other\",\"alpha\"]\n\
[\"two\",\"ten\",1,2,3]\n\
[[\"2\",\"two\"],[\"10\",\"ten\"],[\"tag\",1],[\"other\",2],[\"alpha\",3]]\n\
[\"2\",\"10\",\"tag\",\"other\",\"alpha\"]\n\
[\"2\",\"10\",\"tag\",\"other\",\"alpha\"]\n\
[\"2\",\"10\",\"tag\",\"other\",\"alpha\"]\n\
[\"2\",\"10\",\"tag\",\"alpha\",\"other\"]\n\
[\"2\",\"10\",\"tag\",\"alpha\",\"other\"]\n"
    );
}
