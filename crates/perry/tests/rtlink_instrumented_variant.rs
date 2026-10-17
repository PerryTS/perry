//! rtlink step 3: the default archives carry no GC/diagnostic instruments, and
//! perry links an instrumented prebuilt variant instead when an instrument knob
//! is set while compiling (`optimized_libs::instrumented_variant_requested`).
//! The user-facing diagnostics (heap snapshots, `process.report`) are not
//! instruments: every archive carries them.
//!
//! The instrumented archives are built by `scripts/build_instr_runtime.sh`
//! (CI builds them next to the default ones).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Every environment variable that selects the instrumented variant.
const VARIANT_KNOBS: &[&str] = &[
    "PERRY_GC_INSTRUMENTS",
    "PERRY_GC_TRACE",
    "PERRY_CHECK_CONTRACTS",
    "PERRY_GC_SCHEDULE_SEED",
    "PERRY_GC_SCHEDULE_RATE",
    "PERRY_GC_CENSUS",
    "PERRY_GC_PROTECT_FROMSPACE",
    "PERRY_GC_FROMSPACE_SCAN",
    "PERRY_GC_VERIFY_MARK",
    "PERRY_REGEX_DIAG",
    "PERRY_IC_DIAG",
    "PERRY_LAYOUT_DIAG",
    "PERRY_ENUM_DIAG",
    "PERRY_RECEIVER_REPR_DIAG",
];

fn compile(dir: &Path, source: &str, knob: Option<(&str, &str)>) -> PathBuf {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");
    let mut cmd = Command::new(perry_bin());
    cmd.current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache");
    for name in VARIANT_KNOBS {
        cmd.env_remove(name);
    }
    if let Some((name, value)) = knob {
        cmd.env(name, value);
    }
    let out = cmd.output().expect("run perry compile");
    assert!(
        out.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    output
}

fn run(binary: &Path, knob: Option<(&str, &str)>) -> Output {
    let mut cmd = Command::new(binary);
    for name in VARIANT_KNOBS {
        cmd.env_remove(name);
    }
    if let Some((name, value)) = knob {
        cmd.env(name, value);
    }
    cmd.output().expect("run compiled binary")
}

const GARBAGE: &str = r#"
class P { x: number; constructor(x: number) { this.x = x; } }
let sum = 0;
for (let i = 0; i < 20000; i++) { sum += new P(i).x; }
console.log("sum " + sum);
"#;

#[test]
fn heap_snapshot_and_process_report_work_in_the_default_archives() {
    let dir = tempfile::tempdir().expect("tempdir");
    let snapshot = dir.path().join("out.heapsnapshot");
    let source = format!(
        r#"
import * as v8 from "node:v8";
import * as fs from "node:fs";
const file = (v8 as any).writeHeapSnapshot({path:?});
const doc = JSON.parse(fs.readFileSync(file, "utf8"));
console.log("snapshot", Array.isArray(doc.nodes) && doc.nodes.length > 0);
const report = (process as any).report.getReport();
console.log("report", typeof report.header, typeof report.javascriptHeap);
"#,
        path = snapshot.to_string_lossy()
    );
    let binary = compile(dir.path(), &source, None);
    let out = run(&binary, None);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "snapshot true\nreport object object\n"
    );
}

#[test]
fn the_default_archives_refuse_an_instrument_knob_at_run_time() {
    let dir = tempfile::tempdir().expect("tempdir");
    let binary = compile(dir.path(), GARBAGE, None);
    let out = run(&binary, Some(("PERRY_GC_SCHEDULE_SEED", "7")));
    assert!(
        !out.status.success(),
        "the knob must not silently do nothing"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("built without the GC instruments"),
        "{out:?}"
    );
}

#[test]
fn an_instrument_knob_at_compile_time_links_the_instrumented_archives() {
    let dir = tempfile::tempdir().expect("tempdir");
    let binary = compile(dir.path(), GARBAGE, Some(("PERRY_GC_SCHEDULE_SEED", "7")));
    let out = run(&binary, Some(("PERRY_GC_SCHEDULE_SEED", "7")));
    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("[gc-schedule] done: seed=7"),
        "the seeded schedule must run in the instrumented variant: {out:?}"
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "sum 199990000\n");
}
