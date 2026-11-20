//! The lifetime census (`PERRY_LIFE_CENSUS`) writes its rows from codegen,
//! so a compile served from the object cache or the build cache would leave
//! it silently empty. With the census set, a recompile of an unchanged,
//! already cached program still runs codegen and writes every row.
#![cfg(unix)]

use std::fs;
use std::path::Path;
use std::process::Command;

const SRC: &str = "class P { x: number; constructor(x: number) { this.x = x; } }\n\
function f(i: number): number { const p = new P(i); return p.x; }\n\
console.log(f(1));\n";

fn compile(project: &Path, census: Option<&Path>) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_perry"));
    cmd.current_dir(project)
        .args(["compile", "main.ts", "-o", "main"])
        .arg("--cache-dir")
        .arg(project.join("cache"))
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env_remove("PERRY_NO_CACHE")
        .env_remove("PERRY_DISABLE_BUILD_CACHE")
        .env_remove("PERRY_LIFE_CENSUS");
    if let Some(path) = census {
        cmd.env("PERRY_LIFE_CENSUS", path);
    }
    let out = cmd.output().expect("run perry compile");
    assert!(
        out.status.success(),
        "compile failed: {}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_cached_program_still_writes_its_census() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path();
    fs::write(
        project.join("package.json"),
        "{\"name\":\"life-census-cache\"}\n",
    )
    .unwrap();
    fs::write(project.join("main.ts"), SRC).unwrap();
    // Warm both caches without the census, then compile the same program
    // again with it.
    compile(project, None);
    compile(project, None);
    assert!(
        project.join("cache").join("objects").is_dir(),
        "the first compiles must populate the object cache, or the test proves nothing"
    );
    let census = project.join("census.tsv");
    compile(project, Some(&census));
    let rows = fs::read_to_string(&census).unwrap_or_default();
    assert!(
        rows.lines().any(|l| l.contains("\tP\t")),
        "census on a cached program wrote no row for `new P`: {rows:?}"
    );
}
