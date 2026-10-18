//! `Object.setPrototypeOf` on a prototype an instance already inherits through
//! (a declared class prototype, an `Object.create` hop, a constructor
//! function's prototype): every read, at a site that primed before the relink
//! and at a fresh one, answers from the new chain. The program and node's
//! output are the `one_shape_class_proto_relink` fixture.
//!
//! Both pre-relink and fresh sites must answer from the live prototype chain.

use std::path::PathBuf;
use std::process::Command;

const SOURCE: &str = include_str!("../../../tests/fixtures/one_shape_class_proto_relink/main.ts");
const EXPECTED: &str =
    include_str!("../../../tests/fixtures/one_shape_class_proto_relink/expected.txt");

#[test]
fn reads_follow_a_relinked_prototype() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, SOURCE).expect("write entry");
    let compile = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_perry")))
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_GC_INSTRUMENTS", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    // Runtime trip counts: a fixed small loop is unrolled and its site never
    // reached. The output does not depend on the count.
    for n in ["40", "41"] {
        let run = Command::new(&output)
            .arg(n)
            .current_dir(dir.path())
            .env("PERRY_GC_FORCE_EVACUATE", "1")
            .env("PERRY_GC_VERIFY_EVACUATION", "1")
            .output()
            .expect("run compiled binary");
        let stdout = String::from_utf8_lossy(&run.stdout);
        let stderr = String::from_utf8_lossy(&run.stderr);
        assert!(
            run.status.success(),
            "n={n}: binary failed ({:?})\nstderr:\n{stderr}",
            run.status
        );
        let wrong: Vec<String> = EXPECTED
            .lines()
            .zip(stdout.lines())
            .filter(|(want, got)| want != got)
            .map(|(want, got)| format!("got `{got}`, node `{want}`"))
            .collect();
        assert!(
            wrong.is_empty() && EXPECTED.lines().count() == stdout.lines().count(),
            "n={n}: stale reads {wrong:?}\n{stdout}"
        );
    }
}
