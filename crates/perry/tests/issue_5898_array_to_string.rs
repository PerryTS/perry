//! Regression coverage for the Array.prototype.toString subcluster in #5898.
//! The method must remain reflective through `.call`, dispatch a callable
//! `join`, and fall back to the Object.prototype.toString intrinsic otherwise.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn array_to_string_uses_the_live_prototype_method_and_generic_receiver() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        include_str!("fixtures/issue_5898_array_to_string.ts"),
    )
    .expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
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
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        concat!(
            "true\n",
            "[object Array]\n",
            "[object Array]\n",
            "[object Array]\n",
            "[object Array]\n",
            "[object Array]\n",
            "custom toString\n",
            "[object Boolean]\n",
            "[object Boolean]\n",
            "custom join\n",
            "[object Object]\n",
        )
    );
}
