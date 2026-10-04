//! A module split into codegen units must still LINK and RUN.
//!
//! A large module is compiled as several codegen units that are merged into
//! one object with a relocatable link (`ld -r`). Apple's ld64 turns
//! private-extern (hidden) symbols into local ones in that merge unless it is
//! passed `-keep_private_externs`. ConstFn bodies' `$info` records are emitted
//! `hidden` precisely because a separate object, the static shape-seed table,
//! references them; after the merge demoted them, every split module that
//! carried a ConstFn method failed to link with `Undefined symbols … $info`.
//! The 13 MB Claude Code bundle (128 units) failed with 5,668 of them.
//!
//! The existing split-unit tests compile with `--no-link`, so they could not
//! see a failure that only happens at the final link. This one links and runs.

use std::path::{Path, PathBuf};
use std::process::Command;

const SOURCE: &str = r#"
function mk(n: number) { return (x: number) => x + n; }
function mk2(n: number) { return { add(x: number) { return x + n; }, sub(x: number) { return x - n; } }; }
function a() { const f = mk(1); return f(1); }
function b() { const o = mk2(2); return o.add(3) + o.sub(1); }
function c() { const g = (y: number) => y * 2; return [1, 2, 3].map(g).reduce((p, q) => p + q, 0); }
function d() { return [a, b, c].map(fn => fn()); }
console.log(JSON.stringify(d()));
"#;

fn compile_and_run(dir: &Path, units: &str) -> String {
    let entry = dir.join("main.ts");
    let output: PathBuf = dir.join(format!("main_{units}"));
    std::fs::write(&entry, SOURCE).expect("write entry");
    let compile = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .env("PERRY_CODEGEN_UNITS", units)
        .env("PERRY_NO_CACHE", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "compile with PERRY_CODEGEN_UNITS={units} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&output)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(run.status.success(), "binary failed: {:?}", run.status);
    String::from_utf8_lossy(&run.stdout).trim().to_string()
}

#[test]
fn a_split_module_links_and_runs_like_an_unsplit_one() {
    let dir = tempfile::tempdir().expect("temporary directory");
    // The unsplit build is the control: it never went through the merge.
    assert_eq!(compile_and_run(dir.path(), "1"), "[2,4,12]");
    // The forced split is what the large bundles take.
    assert_eq!(compile_and_run(dir.path(), "4"), "[2,4,12]");
}
