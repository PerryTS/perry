//! Regression test for #7024: under an explicit `PERRY_GC_HEAP_LIMIT`, a
//! compiled program must still run the **copying** minor.
//!
//! `gc_check_trigger`'s deferral arm hands an allocation-point nursery trigger
//! to the next precise-root safepoint, which is the only way the copying
//! (relocating) young-gen minor #7019 shipped ever runs on the automatic path.
//! That deferral used to be guarded by an absolute committed-arena cap derived
//! from `budget_scaled(128 MB, 1, 4, 2 MB)` — byte-for-byte the formula behind
//! `gc_trigger_absolute_ceiling_bytes()`. Under any heap budget small enough
//! for the ceiling to reach the 16 MB nursery cap, the two collapse to one
//! number, and since a nursery trigger is due exactly when
//! `arena_total_bytes() >= trigger` while the deferral required
//! `arena_total_bytes() < cap`, the two predicates became exact complements:
//! the deferral was unreachable, control fell through to the alloc-point minor
//! under `ManualGcScanGuard::force_full_scan()`, and the collector reported
//!
//! ```text
//! [gc-copy-minor] eligible=false fallback=conservative_stack
//! ```
//!
//! for the whole run. Measured on the representation corpus at
//! `PERRY_GC_HEAP_LIMIT=8` before the fix: **0 of 22 files ran a single copying
//! minor**, while the same 22 files collected on 13. So a heap-limited
//! deployment — every small container, every watch-class device, and every arm
//! of the GC stress matrix that uses the pressure knob — silently ran the
//! *pre*-#7019 non-moving collector.
//!
//! What this test pins is the observable that distinguishes those two worlds:
//! a non-in-place copying minor relocated at least one object. Asserting "a GC
//! cycle happened" does not — the broken build collected too. Exit 0 does not
//! either; the broken build exited 0.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Sum objects relocated by `[gc-copy-minor] ran ...` records in the
/// collector's `PERRY_GC_DIAG` output. This uses the copying minor's OWN
/// `copied_objects` and `promoted_objects` counters: the
/// `moved_objects=` counter that also appears there belongs to the C4b
/// evacuation policy inside the mark-sweep collector, a different collector
/// entirely, and summing the two is how a green result was once reported for a
/// run that scavenged nothing (#7025).
///
/// The diagnostic is a key/value record, not a positional format. Ordinary
/// object-by-object promotion relocates survivors just like a nursery copy;
/// whole-block `in_place=true` promotion deliberately does not move them.
fn copy_minor_relocated_objects(stderr: &str) -> u64 {
    stderr
        .lines()
        .filter_map(|line| line.strip_prefix("[gc-copy-minor] ran "))
        .map(|fields| {
            let mut in_place = false;
            let mut copied = 0;
            let mut promoted = 0;

            for field in fields.split_whitespace() {
                let Some((key, value)) = field.split_once('=') else {
                    continue;
                };
                match key {
                    "in_place" => in_place = value == "true",
                    "copied_objects" => copied = value.parse::<u64>().unwrap_or(0),
                    "promoted_objects" => promoted = value.parse::<u64>().unwrap_or(0),
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

#[test]
fn copy_minor_parser_reads_current_field_order() {
    let stderr = "[gc-copy-minor] ran in_place=false survival_permille=350 copied_objects=17 copied_bytes=272 promoted_objects=0 promoted_bytes=0\n";
    assert_eq!(copy_minor_relocated_objects(stderr), 17);
}

#[test]
fn copy_minor_parser_counts_object_by_object_promotions() {
    let stderr = "[gc-copy-minor] ran in_place=false copied_objects=0 promoted_objects=23 promoted_bytes=368\n";
    assert_eq!(copy_minor_relocated_objects(stderr), 23);
}

#[test]
fn copy_minor_parser_rejects_in_place_promotion() {
    let stderr = "[gc-copy-minor] ran in_place=true copied_objects=0 promoted_objects=999 promoted_bytes=15984\n";
    assert_eq!(copy_minor_relocated_objects(stderr), 0);
}

#[test]
fn copying_minor_runs_under_an_explicit_heap_limit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    // Escaping allocation churn in a plain `for` loop: the body allocates, so
    // codegen emits the loop back-edge poll (`js_gc_loop_safepoint`) that
    // drains the deferral, and the sink keeps the objects genuinely live for a
    // while so survivors exist for the copying minor to relocate.
    std::fs::write(
        &entry,
        r#"
let sink: any[] = [];
let checksum = 0;
for (let i = 0; i < 400000; i++) {
  sink.push({ i, s: "x" + (i & 255), pair: { a: i, b: i + 1 } });
  if (sink.length > 2048) {
    checksum = (checksum + sink.length) | 0;
    sink = [];
  }
}
console.log("checksum:", checksum);
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
        // 8 MB is the pressure setting `scripts/gc_repsel_matrix.sh` uses, and
        // the one on which the copying minor was measured to never run.
        .env("PERRY_GC_HEAP_LIMIT", "8")
        .env("PERRY_GC_DIAG", "1")
        .output()
        .expect("run compiled binary");
    let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
    assert!(
        run.status.success(),
        "compiled binary failed (exit {:?})\nstderr:\n{stderr}",
        run.status.code(),
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        // Verified against the pinned Node oracle (`.node-version`).
        "checksum: 399555\n",
        "the workload must still produce its result under a heap limit"
    );

    let relocated = copy_minor_relocated_objects(&stderr);
    assert!(
        relocated > 0,
        "no copying minor ran under PERRY_GC_HEAP_LIMIT=8 (#7024). The \
         allocation-point deferral to the precise-root safepoint is \
         unreachable again — check that the moving-defer allowance is still a \
         SLACK measured from the deferral point and has not been turned back \
         into an absolute arena cap sharing a formula with \
         gc_trigger_absolute_ceiling_bytes().\ncollector diagnostics:\n{stderr}"
    );
}
