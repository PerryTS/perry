//! Regression test: the nursery allocation trigger must actually fire in
//! COMPILED programs.
//!
//! Since the budgeted-GC slicing (2026-05-30), `gc_check_trigger` routed
//! ArenaBytes/MallocCount pressure exclusively through the budgeted
//! mutator-assist stepper — which `registered_root_scanners_block_budgeted_gc`
//! permanently blocks in every compiled program (codegen registers
//! synchronous-only root scanners at startup). Result: a compiled program
//! that churned small allocations NEVER ran a collection cycle — 4M small
//! objects grew RSS to 1.9 GB with zero cycles, and a weak-only-reachable
//! target was never collected, so `WeakRef.deref()` never cleared. #5476
//! had patched the same hole for the OldReclaim trigger only; the direct
//! synchronous-minor arm now covers the nursery-churn triggers too.
//!
//! The observable here: after ~200 MB of allocation churn with the target
//! dead, `wr.deref()` must return undefined. Under the broken trigger this
//! deterministically printed "still-alive" (no cycle ever ran); with the
//! fix the arena trigger fires every ~64 MB and the minor collects the
//! target. NOTE this is a Perry-semantics regression test, not a parity
//! test: the spec guarantees nothing about weak clearing timing, and node
//! happens to print "still-alive" here (V8's conservative stack scan
//! retains the target). What we pin is OUR contract — allocation pressure
//! must produce collection cycles in a compiled binary.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn nursery_pressure_triggers_collection_and_weakref_clears() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
function makeTarget(): object {
  return { payload: "p".repeat(64) };
}
let t: object | null = makeTarget();
const wr = new WeakRef(t as object);
t = null;
let sink: any[] = [];
for (let i = 0; i < 4000000; i++) {
  sink.push({ i, s: "x" + i });
  if (sink.length > 512) sink = [];
}
console.log("deref-after-pressure:", wr.deref() === undefined ? "cleared" : "still-alive");
"#,
    )
    .expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir.path())
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed (exit {:?})\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "deref-after-pressure: cleared\n",
        "the nursery trigger must fire under allocation pressure and the \
         dead weak target must be collected (\"still-alive\" = no GC cycle \
         ever ran in the compiled binary)"
    );
}
