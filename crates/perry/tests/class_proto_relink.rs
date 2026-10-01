//! `Object.setPrototypeOf` on a prototype an instance already inherits through
//! (a declared class prototype, an `Object.create` hop, a constructor
//! function's prototype): every read, at a site that primed before the relink
//! and at a fresh one, answers from the new chain. The program and node's
//! output are the `one_shape_class_proto_relink` fixture.
//!
//! The declared-class reads used to walk the parent class id registered at
//! declaration and answer from the old parent's prototype. A2's class read
//! site confirms its prime against that read, so it declined and the stale
//! value went out; on main the deleted inherited-read cache masked it once a
//! site had primed.

use std::path::PathBuf;
use std::process::Command;

const SOURCE: &str = include_str!("../../../tests/fixtures/one_shape_class_proto_relink/main.ts");
const EXPECTED: &str =
    include_str!("../../../tests/fixtures/one_shape_class_proto_relink/expected.txt");

fn stat(stderr: &str, name: &str) -> u64 {
    let line = stderr
        .lines()
        .find(|l| l.starts_with("[method-site]"))
        .unwrap_or_else(|| panic!("no [method-site] line in:\n{stderr}"));
    line.split_whitespace()
        .find_map(|w| w.strip_prefix(name).and_then(|v| v.strip_prefix('=')))
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| panic!("no {name} in {line}"))
}

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
            .env("PERRY_METHOD_SITE_STATS", "1")
            .env("PERRY_GC_FORCE_EVACUATE", "1")
            .env("PERRY_GC_POISON_FROMSPACE", "1")
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
        // The sites must have primed before the relinks, or nothing here
        // tested what they validate.
        assert!(
            stat(&stderr, "class_read_primes") > 0,
            "n={n}: no class read entry primed\n{stderr}"
        );
        assert!(
            stat(&stderr, "read_holder_primes") > 0,
            "n={n}: no holder entry primed\n{stderr}"
        );
    }
}
