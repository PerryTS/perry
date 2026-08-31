//! Regression test for #9234: a budgeted GC cycle must not reach the native
//! root scan with an unbuilt stack-map index.
//!
//! #9191 made the stack-map index lazy — built on first collection instead of
//! at startup — and wired the four `gc_collect_*` entry points by hand. #9231
//! then found that BUDGETED cycles construct `GcCycleState` directly and enter
//! through none of them, so the first root-scan step hit #9182's fail-closed
//! guard and aborted; #9233 wired those two sites, by hand again.
//!
//! What this pins is the observable, not the wiring: an allocating program
//! under `PERRY_GC_HEAP_LIMIT=8 PERRY_GC_FORCE_EVACUATE=1` runs to completion
//! with the right answer. Those two knobs together are what drive the budgeted
//! path; neither alone reproduced it.
//!
//! Exit 0 is deliberately NOT the only assertion. The failure aborts with
//! SIGABRT *after* the program has already printed its correct result, so a
//! test that only compared stdout would pass against the broken build. The
//! status and an empty stderr are what distinguish the two worlds.
//!
//! The guard now also runs inside `GcCycleState::new_full` /
//! `new_minor_fallback`, which every cycle passes through, so a future entry
//! point is correct by construction rather than by remembering. Verified to
//! discriminate: with that constructor call removed AND the two `policy.rs`
//! entry calls removed, this test aborts with
//! "the native root scan ran before the stack-map index was built".

use std::path::PathBuf;
use std::process::Command;

// `Command` inherits the test runner's environment, and several of these knobs
// change generated code as well as collector policy. Clear the family from
// both subprocesses before applying this test's intended arm.
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
];

fn remove_gc_env_overrides(command: &mut Command) {
    for key in GC_ENV_OVERRIDES {
        command.env_remove(key);
    }
}

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Allocation churn against a live array, so the collector has both garbage to
/// reclaim and a survivor set to relocate.
const SOURCE: &str = r#"
const arr: number[] = [];
for (let i = 0; i < 1024; i++) arr.push(i * 2);
const sink: object[] = [];
function churn(): number { let s = 0; for (let i = 0; i < arr.length; i++) s += arr[i]; return s; }
let out = 0;
for (let k = 0; k < 200; k++) {
  sink.push({ a: k, b: "x" + k });
  if (sink.length > 50) sink.length = 0;
  out = churn();
}
let check = 0;
for (let i = 0; i < arr.length; i++) check += arr[i];
console.log(out + " " + check);
"#;

#[test]
fn a_budgeted_cycle_builds_the_stack_map_index_before_scanning_roots() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, SOURCE).expect("write entry");

    let mut compile_command = Command::new(perry_bin());
    compile_command
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache");
    remove_gc_env_overrides(&mut compile_command);
    let compile = compile_command.output().expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let mut run_command = Command::new(&output);
    run_command.current_dir(dir.path());
    remove_gc_env_overrides(&mut run_command);
    let run = run_command
        .env("PERRY_GC_HEAP_LIMIT", "8")
        .env("PERRY_GC_FORCE_EVACUATE", "1")
        .output()
        .expect("run compiled binary");

    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        !stderr.contains("the native root scan ran before the stack-map index was built"),
        "a budgeted cycle reached the root scan with an unbuilt index\nstderr:\n{stderr}"
    );
    // The abort happens AFTER the program prints, so the status carries the
    // signal that stdout does not.
    assert!(
        run.status.success(),
        "compiled binary did not exit cleanly (exit {:?})\nstderr:\n{stderr}",
        run.status
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "1047552 1047552",
        "wrong result under the heap-limited evacuating arm\nstderr:\n{stderr}"
    );
}
