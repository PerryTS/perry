//! Regression test for the fs options-object crash fixed alongside #8122.
//!
//! `mkdirSync(path, { recursive: true })` passes an object where the runtime
//! also accepts a string mode. Before #8204, the generic string helper accepted
//! any plausible NaN-boxed pointer and read the options object's ShapeId as a
//! `StringHeader::byte_len`. ShapeIds have the high bit set, so the runtime
//! attempted a roughly 2 GiB string copy and crashed in `memcpy`.
//!
//! Claude Code exercises this exact call shape during normal startup, while its
//! `--version` fast path exits before touching the filesystem.

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const SOURCE: &str = r#"
import * as fs from "node:fs";

const root = ".perry-issue-8122-mkdir";
fs.mkdirSync(root + "/a/b", { recursive: true });
const ok = fs.statSync(root + "/a/b").isDirectory();
console.log(ok ? "PASS" : "FAIL");
fs.rmSync(root, { recursive: true, force: true });
"#;

fn compile(dir: &Path, entry: &Path, output: &Path) {
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(entry)
        .arg("-o")
        .arg(output)
        .arg("--no-cache")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
}

#[test]
fn recursive_mkdir_options_object_is_not_decoded_as_a_string() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, SOURCE).expect("write entry");

    compile(dir.path(), &entry, &output);

    let run = Command::new(&output)
        .current_dir(dir.path())
        .output()
        .expect("run compiled binary");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success() && stdout.contains("PASS"),
        "recursive mkdir with an options object failed (status {:?})\nstdout:\n{}\nstderr:\n{}",
        run.status,
        stdout,
        String::from_utf8_lossy(&run.stderr)
    );
}
