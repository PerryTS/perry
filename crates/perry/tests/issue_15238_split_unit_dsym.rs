//! Under `--debug-symbols` a module split into codegen units must reach the
//! `.dSYM` like an unsplit one, cold and from a warm object cache.

#![cfg(target_os = "macos")]

use std::path::Path;
use std::process::Command;

const SOURCE: &str = r#"
function mk(n: number) { return (x: number) => x + n; }
function a() { const f = mk(1); return f(1); }
function b() { return [1, 2, 3].map((y) => y * 2).join(","); }
function c() { return JSON.stringify({ k: a(), m: b() }); }
console.log(c());
"#;

fn compile(dir: &Path, units: &str, output: &str, flags: &[&str]) -> (String, String) {
    let exe = dir.join(output);
    let compile = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(dir)
        .args(["compile", "main.ts", "--debug-symbols", "-o"])
        .arg(&exe)
        .args(flags)
        .env("PERRY_CODEGEN_UNITS", units)
        .env("TMPDIR", dir.join("tmp"))
        .output()
        .expect("run perry compile");
    let stderr = String::from_utf8_lossy(&compile.stderr).into_owned();
    assert!(compile.status.success(), "compile failed\n{stderr}");
    let dwarf = Command::new("dwarfdump")
        .arg("--debug-line")
        .arg(exe.with_extension("dSYM"))
        .output()
        .expect("run dwarfdump");
    (stderr, String::from_utf8_lossy(&dwarf.stdout).into_owned())
}

#[test]
fn split_units_keep_their_debug_info() {
    let dir = tempfile::tempdir().expect("temporary directory");
    std::fs::create_dir(dir.path().join("tmp")).expect("create TMPDIR");
    std::fs::write(dir.path().join("main.ts"), SOURCE).expect("write entry");

    // A `--no-link` build shares the object cache with the linking builds below.
    compile(dir.path(), "4", "object.o", &["--no-link"]);
    for (units, output) in [("1", "whole"), ("4", "cold"), ("4", "warm")] {
        let (stderr, lines) = compile(dir.path(), units, output, &[]);
        assert!(
            !stderr.contains("unable to open object file"),
            "{output}: {stderr}"
        );
        assert!(
            lines.contains("main.ts"),
            "{output}: no line table\n{stderr}"
        );
    }
    let leaked: Vec<_> = std::fs::read_dir(dir.path().join("tmp"))
        .expect("read TMPDIR")
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.starts_with("perry_cgu_"))
        .collect();
    assert!(
        leaked.is_empty(),
        "unit objects outlive the compile: {leaked:?}"
    );
}
