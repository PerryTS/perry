//! Regression coverage for #9173: Uint8Array shares Perry's BufferHeader
//! storage but not Node's Buffer brand, and Buffer.prototype inherits from
//! Uint8Array.prototype.

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
fn buffer_brand_and_uint8array_prototype_chain_match_node() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = workspace_root().join("test-files/test_issue_9173_buffer_identity.ts");
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
        "buffer brand: true\nbuffer is uint8: true\nuint8 brand: false\nuint8 is uint8: true\narray buffer brand: false\ndata view brand: false\ncaptured brand: true false\nbuffer prototype: true\nbuffer parent prototype: true\nprototype link: true\n"
    );
}
