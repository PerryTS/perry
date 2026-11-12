//! The off arm of `PERRY_REST_TIERS` (I32 at rest B2) is a correct program.
//!
//! `PERRY_REST_TIERS=0` turns the at-rest tiers off in codegen; the Arrays
//! the facts admit at rest are still born with I32 words. A class field
//! holding one kept a double `Arr` representation with the switch off, so
//! the rep-keyed `this.cells[sq] ?? 0` (`expr/index_get/rep_default.rs`)
//! read the I32 words as doubles: `test_rest_loops` printed `field 24 0 ..`
//! and check mode stopped with exit 86, "array rep". The fixture must print
//! node's output with the switch on and off, in default and check mode.

use std::path::PathBuf;
use std::process::Command;

/// node's output of `tests/parity/language/number/test_rest_loops.ts`.
const EXPECTED: &str = "local 415346400 2176 13109652 0 261198\n\
param 704711416 704711412\n\
mixed 88459056\n\
field 24 -1571810952 0 0 0 0 0\n\
json 6048 219\n\
frac 16 22\n";

fn clean(cmd: &mut Command) -> &mut Command {
    for (key, _) in std::env::vars() {
        if key.starts_with("PERRY_") || key == "NODE_OPTIONS" {
            cmd.env_remove(key);
        }
    }
    cmd
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/parity/language/number/test_rest_loops.ts")
}

#[test]
fn rest_loops_matches_node_with_the_rest_tiers_on_and_off_in_default_and_check_mode() {
    let dir = tempfile::tempdir().unwrap();
    let entry = dir.path().join("main.ts");
    std::fs::copy(fixture(), &entry).unwrap();
    for tiers in ["1", "0"] {
        for check in [false, true] {
            let what = format!("PERRY_REST_TIERS={tiers} check={check}");
            let binary = dir.path().join(format!(
                "main_{tiers}_{}",
                if check { "check" } else { "default" }
            ));
            let mut compile = Command::new(env!("CARGO_BIN_EXE_perry"));
            clean(&mut compile)
                .current_dir(dir.path())
                .env("PERRY_NO_CACHE", "1")
                .env("PERRY_REST_TIERS", tiers);
            if check {
                compile.env("PERRY_VERIFY_FACTS", "1");
            }
            let out = compile
                .arg("compile")
                .arg(&entry)
                .arg("-o")
                .arg(&binary)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "compile ({what}): {}",
                String::from_utf8_lossy(&out.stderr)
            );
            let mut run = Command::new(&binary);
            let ran = clean(&mut run).output().unwrap();
            let stderr = String::from_utf8_lossy(&ran.stderr);
            assert!(
                ran.status.success(),
                "run ({what}): {:?}\n{stderr}",
                ran.status
            );
            assert!(!stderr.contains("PERRY_FALSE_PROOF"), "{what}: {stderr}");
            assert_eq!(String::from_utf8_lossy(&ran.stdout), EXPECTED, "{what}");
        }
    }
}
