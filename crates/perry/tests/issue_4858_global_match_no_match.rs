//! Regression test for #4858: `String.prototype.match` with a global (`/g`)
//! regex that finds no match must return `null` — not a POINTER_TAG-boxed
//! null pointer that compares unequal to `null` and segfaults consumers
//! (`JSON.stringify`, `.map`). This was the root cause of the Stripe SDK
//! failure in #4841 (`extractUrlParams` runs `path.match(/\{\w+\}/g)` on
//! every request path).

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn global_match_no_match_returns_null() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");

    std::fs::write(
        &entry,
        r#"
const a = "abc".match(/x/g);
console.log(a === null, JSON.stringify(a));
const b = "abc".match(/x/);
console.log(b === null, JSON.stringify(b));
const stripe = "/v1/products".match(/\{\w+\}/g);
console.log(stripe === null);
const hit = "a1b2c3".match(/\d/g);
console.log(JSON.stringify(hit));
"#,
    )
    .expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
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
        "compiled binary failed (signal/segfault = #4858 regression)\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        stdout, "true null\ntrue null\ntrue\n[\"1\",\"2\",\"3\"]\n",
        "global/non-global no-match must be null; matches must survive"
    );
}
