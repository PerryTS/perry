//! Rematerialized global-backed roots, end to end.
//!
//! `perry-codegen`'s native root lowering no longer relocates a slot whose only
//! heap value is a copy of a string-literal handle or a class-keys global; it
//! re-reads the global at each use (`function/precise_roots/remat.rs`). This
//! test compiles `test-files/test_gap_remat_global_backed_roots.ts` once and
//! runs it under several collector configurations, each of which must print the
//! node oracle.
//!
//! It cannot pass vacuously in either of the two ways that matter:
//!
//! - **The subject is in the artifact.** The `--trace llvm` output must contain
//!   both a rematerialized string-handle read and a rematerialized class-keys
//!   read, or the program never exercised the transform.
//! - **Objects actually moved.** The forced-evacuation arm must report copying
//!   minors that relocated objects (`[gc-copy-minor] ran` records, the copying
//!   minor's own counters; #7025). `PERRY_GC_VERIFY_EVACUATION=1` then panics if
//!   any live mutable slot still points at a forwarded nursery object.
//!
//! Sabotage (performed when this landed, see the changelog fragment): making
//! the rewritten read return the slot's cached address instead of re-loading
//! the global turns the forced-evacuation arm red.

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const SOURCE: &str = include_str!("../../../test-files/test_gap_remat_global_backed_roots.ts");

/// node 26.5.1 (`.node-version`) on the program above.
const NODE_ORACLE: &str = "\
setup-banner-literal|setup-banner-literal|setup-banner-literal|setup-tail|x,y,label|kind,n
hello-remat-literal::0::57000::96120::429::busy::x,y,label/x,y,label/kind,n::the-end
hello-remat-literal::5::57000::96120::429::busy::x,y,label/x,y,label/kind,n::the-end
hello-remat-literal::10::57000::96120::429::busy::x,y,label/x,y,label/kind,n::the-end
hello-remat-literal::15::57000::96120::429::busy::x,y,label/x,y,label/kind,n::the-end
hello-remat-literal::19::57000::96120::429::busy::x,y,label/x,y,label/kind,n::the-end
captured-literal#0 captured-literal#1 captured-literal#2
";

/// Collector knobs a developer's shell could export, which would silently turn
/// an arm into something else. Cleared before every run.
const GC_ENV_OVERRIDES: &[&str] = &[
    "PERRY_GEN_GC",
    "PERRY_GC_SCAVENGE",
    "PERRY_GC_SCAVENGE_NURSERY_MB",
    "PERRY_GC_MOVING_SAFEPOINT",
    "PERRY_GC_MOVING_LOOP_POLLS",
    "PERRY_GC_FORCE_EVACUATE",
    "PERRY_GC_VERIFY_EVACUATION",
    "PERRY_GC_DIAG",
    "PERRY_CONSERVATIVE_STACK_SCAN",
    "PERRY_WRITE_BARRIERS",
    "PERRY_GC_INCREMENTAL",
    "PERRY_GC_HEAP_LIMIT",
    "PERRY_GC_SCHEDULE_SEED",
    "PERRY_GC_SCHEDULE_RATE",
];

/// Objects relocated by the copying minor, from its own `[gc-copy-minor] ran`
/// records (in-place block promotion moves nothing and is not counted).
fn copy_minor_relocated_objects(stderr: &str) -> u64 {
    stderr
        .lines()
        .filter_map(|line| line.strip_prefix("[gc-copy-minor] ran "))
        .map(|fields| {
            let (mut in_place, mut copied, mut promoted) = (false, 0u64, 0u64);
            for field in fields.split_whitespace() {
                match field.split_once('=') {
                    Some(("in_place", v)) => in_place = v == "true",
                    Some(("copied_objects", v)) => copied = v.parse().unwrap_or(0),
                    Some(("promoted_objects", v)) => promoted = v.parse().unwrap_or(0),
                    _ => {}
                }
            }
            if in_place {
                0
            } else {
                copied + promoted
            }
        })
        .sum()
}

fn traced_ir(dir: &Path) -> String {
    let mut all = String::new();
    let trace = dir.join(".perry-trace").join("llvm");
    for entry in std::fs::read_dir(&trace).expect("--trace llvm wrote .perry-trace/llvm") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_some_and(|e| e == "ll") {
            all.push_str(&std::fs::read_to_string(&path).expect("read traced IR"));
        }
    }
    all
}

#[test]
fn global_backed_roots_survive_evacuation_when_rematerialized() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, SOURCE).expect("write entry");

    let mut compile = Command::new(perry_bin());
    compile
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .arg("--trace")
        .arg("llvm");
    for key in GC_ENV_OVERRIDES {
        compile.env_remove(key);
    }
    let compiled = compile.output().expect("run perry compile");
    assert!(
        compiled.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compiled.stdout),
        String::from_utf8_lossy(&compiled.stderr)
    );

    // The subject is live: both global families were rematerialized.
    let ir = traced_ir(dir.path());
    let remat_reads: Vec<&str> = ir.lines().filter(|l| l.contains(".rmg = load ")).collect();
    assert!(
        remat_reads.iter().any(|l| l.contains("load double, ptr @")
            && l.contains(".str.")
            && l.ends_with(".handle")),
        "no rematerialized string-literal read in the traced IR; the test's \
         subject never ran. Rematerialized reads:\n{}",
        remat_reads.join("\n")
    );
    assert!(
        remat_reads
            .iter()
            .any(|l| l.contains("load i64, ptr @perry_class_keys_")),
        "no rematerialized class-keys read in the traced IR; the test's \
         subject never ran. Rematerialized reads:\n{}",
        remat_reads.join("\n")
    );

    let forced: &[(&str, &str)] = &[
        ("PERRY_GC_FORCE_EVACUATE", "1"),
        ("PERRY_GC_VERIFY_EVACUATION", "1"),
        ("PERRY_GC_DIAG", "1"),
    ];
    let mut arms: Vec<(String, Vec<(&str, &str)>, bool)> = vec![
        ("default".into(), vec![], false),
        ("forced-evacuation".into(), forced.to_vec(), true),
    ];
    for mb in ["1", "2", "4"] {
        let mut env = forced.to_vec();
        env.push(("PERRY_GC_SCAVENGE_NURSERY_MB", mb));
        arms.push((format!("forced-evacuation nursery={mb}MB"), env, true));
    }
    // Control: full mark-sweep never moves anything.
    arms.push(("PERRY_GEN_GC=0".into(), vec![("PERRY_GEN_GC", "0")], false));

    for (label, env, must_move) in &arms {
        let mut cmd = Command::new(&output);
        cmd.current_dir(dir.path());
        for key in GC_ENV_OVERRIDES {
            cmd.env_remove(key);
        }
        for (k, v) in env {
            cmd.env(k, v);
        }
        let run = cmd.output().expect("run compiled binary");
        let stderr = String::from_utf8_lossy(&run.stderr);
        assert!(
            run.status.success(),
            "[{label}] compiled binary failed (exit {:?})\nstderr (tail):\n{}",
            run.status.code(),
            stderr.lines().rev().take(40).collect::<Vec<_>>().join("\n")
        );
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            NODE_ORACLE,
            "[{label}] a rematerialized string literal or class-keys array read \
             a stale address"
        );
        if *must_move {
            let moved = copy_minor_relocated_objects(&stderr);
            assert!(
                moved > 0,
                "[{label}] no copying minor relocated anything, so this arm \
                 proved nothing about stale addresses"
            );
        }
    }
}
