//! #8595 entry-outlining transform — end-to-end differential.
//!
//! `PERRY_OUTLINE_ENTRY` rewrites an eligible module-entry body into per-chunk
//! functions, hoisting top-level `let` declarations so cross-chunk state is
//! globalized (via the existing `emit_module_globals` escape rule) and shared
//! across the chunks. This must not change observable behavior — including
//! under a relocating minor, since the cross-chunk objects now live in module
//! globals that a moving collection has to find and rewrite.
//!
//! The same program is compiled twice from identical source:
//!   * `PERRY_OUTLINE_ENTRY` unset — the ordinary single-function entry;
//!   * `PERRY_OUTLINE_ENTRY=1 PERRY_OUTLINE_ENTRY_CHUNK_STMTS=1` — maximum
//!     chunking, so every top-level statement is its own chunk function and the
//!     object lets `a`/`b`/`c` are genuinely defined in one chunk and read in
//!     another.
//! Both run under every moving-collector arm and must produce byte-identical,
//! correct output. If a cross-chunk global were not rooted/rewritten by a
//! relocating minor, only the outlined arm would diverge.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Straight-line, no exports / no control flow / no top-level await, so it is
/// an eligible outlining candidate. `a`/`b`/`c` are heap objects defined in
/// separate chunks and read together in a later chunk.
const SOURCE: &str = r#"
let a = { v: 3 };
let b = { v: 4 };
let c = { v: 5 };
let sum = a.v + b.v + c.v;
console.log("sum:" + sum);
"#;

const EXPECTED: &str = "sum:12\n";

const GC_ENV_OVERRIDES: &[&str] = &[
    "PERRY_GEN_GC",
    "PERRY_GC_SCAVENGE",
    "PERRY_GC_SCAVENGE_NURSERY_MB",
    "PERRY_GC_MOVING_SAFEPOINT",
    "PERRY_GC_MOVING_LOOP_POLLS",
    "PERRY_GC_FORCE_EVACUATE",
    "PERRY_CONSERVATIVE_STACK_SCAN",
    "PERRY_WRITE_BARRIERS",
    "PERRY_GC_INCREMENTAL",
    "PERRY_GC_HEAP_LIMIT",
    // Outlining knobs — cleared so an exported value can't perturb an arm.
    "PERRY_OUTLINE_ENTRY",
    "PERRY_OUTLINE_ENTRY_CHUNK_STMTS",
    "PERRY_OUTLINE_ENTRY_REPORT",
];

fn compile(dir: &std::path::Path, name: &str, source: &str, outline: bool) -> (PathBuf, String) {
    let entry = dir.join(format!("{name}.ts"));
    let output = dir.join(name);
    std::fs::write(&entry, source).expect("write entry");
    let mut cmd = Command::new(perry_bin());
    cmd.current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache");
    for key in GC_ENV_OVERRIDES {
        cmd.env_remove(key);
    }
    if outline {
        cmd.env("PERRY_OUTLINE_ENTRY", "1")
            .env("PERRY_OUTLINE_ENTRY_CHUNK_STMTS", "1")
            .env("RUST_LOG", "debug");
    }
    let out = cmd.output().expect("run perry compile");
    assert!(
        out.status.success(),
        "compile (outline={outline}) failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    (output, String::from_utf8_lossy(&out.stderr).into_owned())
}

fn run_arms(binary: &std::path::Path, dir: &std::path::Path, label: &str, expected: &str) {
    let mut arms: Vec<Vec<(&str, &str)>> = vec![vec![]];
    for mb in ["1", "2", "4"] {
        arms.push(vec![("PERRY_GC_SCAVENGE_NURSERY_MB", mb)]);
    }
    arms.push(vec![("PERRY_GEN_GC", "0")]);
    for arm in &arms {
        let mut cmd = Command::new(binary);
        cmd.current_dir(dir);
        for key in GC_ENV_OVERRIDES {
            cmd.env_remove(key);
        }
        for (k, v) in arm {
            cmd.env(k, v);
        }
        let run = cmd.output().expect("run compiled binary");
        let arm_label = if arm.is_empty() {
            format!("{label}/default")
        } else {
            format!("{label}/{}={}", arm[0].0, arm[0].1)
        };
        assert!(
            run.status.success(),
            "[{arm_label}] binary failed (exit {:?})\nstderr:\n{}",
            run.status.code(),
            String::from_utf8_lossy(&run.stderr),
        );
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            expected,
            "[{arm_label}] wrong output"
        );
    }
}

#[test]
fn outlined_entry_matches_the_single_function_entry_under_a_relocating_minor() {
    let dir = tempfile::tempdir().expect("tempdir");

    let (off_bin, _) = compile(dir.path(), "toy_off", SOURCE, false);
    let (on_bin, on_stderr) = compile(dir.path(), "toy_on", SOURCE, true);

    // The outlined compile must have actually outlined (into multiple chunk
    // functions), or the differential proves nothing. The transform logs the
    // count at debug level.
    assert!(
        on_stderr.contains("outlined entry body of 'toy_on.ts' into")
            && on_stderr.contains("chunk functions"),
        "PERRY_OUTLINE_ENTRY=1 was expected to outline the entry, but the \
         compile did not report it:\nstderr:\n{on_stderr}"
    );

    run_arms(&off_bin, dir.path(), "single-function", EXPECTED);
    run_arms(&on_bin, dir.path(), "outlined", EXPECTED);
}

/// A body with a top-level `if` between relocatable runs: the transform must
/// outline the runs and keep the `if` inline, in order — and the result must
/// still match the single-function build under a relocating minor.
const INTERLEAVE_SOURCE: &str = r#"
let a = { v: 10 };
let b = { v: 20 };
if (a.v < b.v) { console.log("less"); }
let c = { v: 30 };
console.log("total:" + (a.v + b.v + c.v));
"#;

const INTERLEAVE_EXPECTED: &str = "less\ntotal:60\n";

#[test]
fn outlining_interleaves_chunks_around_inline_control_flow() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (off_bin, _) = compile(dir.path(), "int_off", INTERLEAVE_SOURCE, false);
    let (on_bin, on_stderr) = compile(dir.path(), "int_on", INTERLEAVE_SOURCE, true);
    assert!(
        on_stderr.contains("outlined entry body of 'int_on.ts' into")
            && on_stderr.contains("chunk functions"),
        "the interleaved body must still outline:\nstderr:\n{on_stderr}"
    );
    run_arms(&off_bin, dir.path(), "single-function", INTERLEAVE_EXPECTED);
    run_arms(&on_bin, dir.path(), "outlined", INTERLEAVE_EXPECTED);
}
