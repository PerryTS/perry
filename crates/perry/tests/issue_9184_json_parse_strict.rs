//! #9184: the direct JSON parser validates in the same pass that constructs
//! Perry values, allowing the separate serde validation scan to be removed.

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
fn json_parse_matches_node_across_strict_syntax_battery() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = workspace_root().join("test-files/test_issue_9184_json_parse_strict.ts");
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

    let run = Command::new(&output)
        .env("PERRY_JSON_TAPE", "0")
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled program failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "invalid:43/43:0\nvalid:28/28:0\ntyped:SyntaxError:2:1:2\n"
    );
}
