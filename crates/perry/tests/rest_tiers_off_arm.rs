//! The off arm of `PERRY_REST_TIERS` (I32 at rest B2) is a correct program.
//!
//! `PERRY_REST_TIERS=0` turns the at-rest tiers off in codegen; the Arrays
//! the facts admit at rest are still born with I32 words. A class field
//! holding one kept a double `Arr` representation with the switch off, so
//! the rep-keyed `this.cells[sq] ?? 0` (`expr/index_get/rep_default.rs`)
//! read the I32 words as doubles: `test_rest_loops` printed `field 24 0 ..`
//! and check mode stopped with exit 86, "array rep". Seven more element
//! fixtures were wrong the same way. Each must print, with the switch off in
//! default and in check mode, what it prints with the switch on (node's
//! output; pinned here for `test_rest_loops`).

use std::path::{Path, PathBuf};
use std::process::Command;

/// node's output of `tests/parity/language/number/test_rest_loops.ts`.
const REST_LOOPS: &str = "local 415346400 2176 13109652 0 261198\n\
param 704711416 704711412\n\
mixed 88459056\n\
field 24 -1571810952 0 0 0 0 0\n\
json 6048 219\n\
frac 16 22\n";

/// The fixtures (under `tests/parity/language`) whose off arm was wrong.
const FIXTURES: &[&str] = &[
    "number/test_rest_loops.ts",
    "number/test_rest_capacity.ts",
    "number/test_rest_default.ts",
    "number/test_rest_elems.ts",
    "number/test_rest_push_fill.ts",
    "number/test_int32_operands.ts",
    "array/test_key_sign.ts",
    "function/test_p2_signed_index_params.ts",
];

fn clean(cmd: &mut Command) -> &mut Command {
    for (key, _) in std::env::vars() {
        if key.starts_with("PERRY_") || key == "NODE_OPTIONS" {
            cmd.env_remove(key);
        }
    }
    cmd
}

/// Compile `entry` with `env` and run it: its stdout.
fn run(dir: &Path, entry: &Path, tag: &str, env: &[(&str, &str)]) -> String {
    let binary = dir.join(format!("main_{tag}"));
    let mut compile = Command::new(env!("CARGO_BIN_EXE_perry"));
    clean(&mut compile)
        .current_dir(dir)
        .env("PERRY_NO_CACHE", "1")
        .envs(env.iter().copied());
    let out = compile
        .arg("compile")
        .arg(entry)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "compile ({tag}): {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut cmd = Command::new(&binary);
    let ran = clean(&mut cmd).current_dir(dir).output().unwrap();
    let stderr = String::from_utf8_lossy(&ran.stderr);
    assert!(
        ran.status.success(),
        "run ({tag}): {:?}\n{stderr}",
        ran.status
    );
    assert!(!stderr.contains("PERRY_FALSE_PROOF"), "{tag}: {stderr}");
    String::from_utf8_lossy(&ran.stdout).into_owned()
}

#[test]
fn element_fixtures_print_the_same_with_the_rest_tiers_off_in_default_and_check_mode() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/parity/language");
    for fixture in FIXTURES {
        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path().join("main.ts");
        std::fs::copy(root.join(fixture), &entry).unwrap();
        let on = run(dir.path(), &entry, "on", &[]);
        if fixture.ends_with("test_rest_loops.ts") {
            assert_eq!(on, REST_LOOPS, "{fixture} with the tiers on");
        }
        let off = run(dir.path(), &entry, "off", &[("PERRY_REST_TIERS", "0")]);
        assert_eq!(off, on, "{fixture} with PERRY_REST_TIERS=0");
        let check = run(
            dir.path(),
            &entry,
            "off_check",
            &[("PERRY_REST_TIERS", "0"), ("PERRY_VERIFY_FACTS", "1")],
        );
        assert_eq!(check, on, "{fixture} with PERRY_REST_TIERS=0 in check mode");
    }
}
