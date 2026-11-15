//! A warm compile reuses the object cache whatever the module's static shape
//! seeds hold, and a cache entry whose seed sidecar cannot be read is a miss:
//! the module recompiles, the entry is rewritten, and the program links and
//! runs exactly as it did cold.
//!
//! The fixture's `{ j, text }` literal stores the loop counter `j` in an I32
//! lane (`11` in the birth rep, no ConstFn body), the seed kind the sidecar
//! reader used to reject: every warm build of such a program failed to link
//! with `_main` undefined.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const FIXTURE: &str = r#"function build(n: number): any[] {
  const rows: any[] = [];
  for (let j = 0; j < n; j++) rows.push({ j, text: "t" + j });
  return rows;
}
const rows = build(4);
let sum = 0;
for (const row of rows) sum += row.j;
console.log(sum, rows[3].j === 3, rows[3].text);
"#;

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("package.json"),
        "{\"name\":\"seed-sidecar\"}\n",
    )
    .unwrap();
    fs::write(dir.path().join("main.ts"), FIXTURE).unwrap();
    dir
}

/// Compile `main.ts` to `output` against the project's own cache and return
/// the program's stdout. Every call names a new output so the whole-build
/// cache cannot stand in for the object cache.
fn compile_and_run(project: &Path, output: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(project)
        .args(["compile", "main.ts", "-o", output])
        .arg("--cache-dir")
        .arg(project.join("cache"))
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_DISABLE_BUILD_CACHE", "1")
        .env_remove("PERRY_NO_CACHE")
        .env_remove("PERRY_OBJECT_CACHE_BUILD_ID")
        .output()
        .expect("run perry compile");
    assert!(
        out.status.success(),
        "compile to {output} failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let run = Command::new(project.join(output))
        .output()
        .expect("run fixture");
    assert!(run.status.success(), "fixture failed: {run:?}");
    String::from_utf8_lossy(&run.stdout).into_owned()
}

fn seed_sidecars(project: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    for target in fs::read_dir(project.join("cache").join("objects")).expect("object cache") {
        for entry in fs::read_dir(target.unwrap().path()).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|ext| ext == "seeds") {
                result.push(path);
            }
        }
    }
    result.sort();
    result
}

/// The fixture's sidecar and its text; asserts the cold build recorded the
/// I32-lane seed this test is about, so the test cannot pass vacuously.
fn i32_lane_sidecar(project: &Path) -> (PathBuf, String) {
    let sidecars = seed_sidecars(project);
    assert_eq!(sidecars.len(), 1, "one module, one sidecar: {sidecars:?}");
    let text = fs::read_to_string(&sidecars[0]).unwrap();
    assert!(
        text.lines()
            .any(|line| line.ends_with(" 6a007465787400 0x3 -")),
        "the fixture must seed `{{ j, text }}` with an I32 lane:\n{text}"
    );
    (sidecars[0].clone(), text)
}

#[test]
fn i32_lane_seed_links_warm_with_the_cold_output() {
    let dir = project();
    let cold = compile_and_run(dir.path(), "cold");
    assert_eq!(cold, "6 true t3\n");
    let (sidecar, recorded) = i32_lane_sidecar(dir.path());
    assert_eq!(compile_and_run(dir.path(), "warm"), cold);
    assert_eq!(compile_and_run(dir.path(), "warm2"), cold);
    assert_eq!(fs::read_to_string(sidecar).unwrap(), recorded);
}

#[test]
fn unreadable_seed_sidecar_is_a_miss_and_is_rewritten() {
    let dir = project();
    let cold = compile_and_run(dir.path(), "cold");
    let (sidecar, recorded) = i32_lane_sidecar(dir.path());
    let truncated = &recorded[..recorded.len() / 2];
    let flipped = recorded.replacen("0x3", "0x\u{1}", 1);
    for (name, damaged) in [("truncated", truncated), ("flipped", flipped.as_str())] {
        fs::write(&sidecar, damaged).unwrap();
        assert_eq!(compile_and_run(dir.path(), name), cold, "{name} sidecar");
        assert_eq!(
            fs::read_to_string(&sidecar).unwrap(),
            recorded,
            "the {name} entry is rewritten by the recompile"
        );
    }
}
