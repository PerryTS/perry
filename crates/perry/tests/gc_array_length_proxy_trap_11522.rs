//! Regression test for #11522 — `js_array_length` was classified
//! allocate-but-never-reenter, but its #5135 Proxy arm runs the user's `get`
//! trap (and its array-like-object arm runs getters plus `valueOf`).
//!
//! Under `PERRY_GC_SAFEPOINT_ONLY` an `AllocNoReentry` callee is marked
//! `"gc-leaf-function"`, so RS4GC records no relocations for the caller's live
//! GC values across the call. A trap that allocates and runs `gc()` is a
//! DECLARED safepoint — it collects on precise roots, and under
//! `PERRY_GC_FORCE_EVACUATE=1` it moves every marked nursery survivor — so the
//! caller came back holding from-space pointers.
//!
//! The fix splits the helper: `js_array_length_leaf` is the header-read fast
//! lane (a real leaf), and only its `-1` miss takes the ordinary
//! `js_array_length` call, now classified Unknown so it gets a statepoint.
//!
//! `js_array_push_f64` had the identical defect (its #5135 arm runs the
//! `get("length")` and `set` traps) and is now Unknown as well.
//!
//! The program pushes onto a `number[]`-typed Proxy and consumes the push's
//! result, so one statement crosses both helpers: `js_array_push_f64` runs
//! the traps, then the result lowers through `js_array_length_leaf` → miss →
//! `js_array_length`, which runs the `length` trap again. Phase 1 collects in
//! the first `length` read (push), phase 2 in the second (the length helper);
//! a survivor that has moved once is promoted and does not move again, so a
//! single phase would only ever test whichever helper collects first. The
//! collecting trap churns
//! enough nursery to arm the arena trigger, so a loop back-edge poll inside it
//! runs a copying minor at a DECLARED safepoint (an explicit `gc()` here would
//! not move: the contract heals it with a conservative scan). The caller holds
//! a freshly allocated object and an array inside it across the push and reads
//! both afterwards. Before the fix they came back stale (`live1:[object
//! Object]:5`). The run must also prove its subject was live: at least one
//! copying minor relocated objects.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const SOURCE: &str = r#"
let lengthReads = 0;
// Which of a push's two `length` reads collects: 1 = the read inside
// js_array_push_f64, 2 = the read through js_array_length for the result.
// A survivor that has moved once is promoted and never moves again, so each
// half needs a phase in which it is the FIRST collection `live` sees.
let collectOnRead = 1;
function churnAndCollect(): void {
  let sink = 0;
  for (let i = 0; i < 200000; i++) {
    const tmp = { i, s: "pad" + i };
    sink += tmp.s.length > 0 ? 1 : 0;
  }
  (globalThis as any).gc?.();
  for (let i = 0; i < 200000; i++) {
    const tmp2 = { a: i, b: "fill" + i, c: [i, i + 1] };
    sink += tmp2.c[0] >= 0 ? 1 : 0;
  }
  if (sink !== 400000) throw new Error("churn miscounted");
}

const target: number[] = [1, 2, 3];
const p: number[] = new Proxy(target, {
  get(t: any, k: any, r: any) {
    if (k === "length") {
      lengthReads++;
      if (lengthReads % 2 === collectOnRead % 2) churnAndCollect();
    }
    return Reflect.get(t, k, r);
  },
});

function run(round: number): string {
  const live = { tag: "live" + round, items: [round, round + 1, round + 2] };
  const n = p.push(round);
  return live.tag + ":" + live.items.join(",") + ":" + n;
}

for (let r = 0; r < 3; r++) console.log(run(r));
collectOnRead = 2;
for (let r = 3; r < 6; r++) console.log(run(r));
console.log("target", target.join(","), "reads>0", lengthReads > 0);
"#;

/// `node --experimental-strip-types` output for `SOURCE`.
const EXPECTED: &str = "live0:0,1,2:4\n\
live1:1,2,3:5\n\
live2:2,3,4:6\n\
live3:3,4,5:7\n\
live4:4,5,6:8\n\
live5:5,6,7:9\n\
target 1,2,3,0,1,2,3,4,5 reads>0 true\n";

/// Objects relocated by `[gc-copy-minor] ran …` records: the copying minor's
/// own `copied_objects` + `promoted_objects`, excluding in-place promotion
/// (see `gc_copy_minor_under_heap_limit.rs` for why not `moved_objects`).
fn copy_minor_relocated_objects(stderr: &str) -> u64 {
    stderr
        .lines()
        .filter_map(|line| line.strip_prefix("[gc-copy-minor] ran "))
        .map(|fields| {
            let (mut in_place, mut copied, mut promoted) = (false, 0, 0);
            for field in fields.split_whitespace() {
                match field.split_once('=') {
                    Some(("in_place", v)) => in_place = v == "true",
                    Some(("copied_objects", v)) => copied = v.parse::<u64>().unwrap_or(0),
                    Some(("promoted_objects", v)) => promoted = v.parse::<u64>().unwrap_or(0),
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
fn proxy_length_trap_collection_relocates_caller_roots() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, SOURCE).expect("write entry");

    let mut compile_cmd = Command::new(perry_bin());
    compile_cmd
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        // The contract that turns AllocNoReentry callees into gc-leaf calls:
        // without it the misclassification was harmless.
        .env("PERRY_GC_SAFEPOINT_ONLY", "1");
    if cfg!(target_os = "macos") {
        let extra = match std::env::var("PERRY_EXTRA_LINK_ARGS") {
            Ok(existing) if !existing.trim().is_empty() => {
                format!("{existing} -framework CoreFoundation")
            }
            _ => "-framework CoreFoundation".to_string(),
        };
        compile_cmd.env("PERRY_EXTRA_LINK_ARGS", extra);
    }
    let compile = compile_cmd.output().expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir.path())
        .env("PERRY_GC_SAFEPOINT_ONLY", "1")
        .env("PERRY_GC_FORCE_EVACUATE", "1")
        .env("PERRY_GC_VERIFY_EVACUATION", "1")
        .env("PERRY_GC_DIAG", "1")
        .output()
        .expect("run compiled binary");
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        run.status.success(),
        "binary failed under forced evacuation (exit {:?})\nstdout:\n{stdout}\nstderr (tail):\n{}",
        run.status.code(),
        stderr.lines().rev().take(40).collect::<Vec<_>>().join("\n")
    );
    assert_eq!(stdout, EXPECTED, "output diverged from node");
    let relocated = copy_minor_relocated_objects(&stderr);
    assert!(
        relocated > 0,
        "no copying minor relocated anything: the test did not exercise a moving collection"
    );
}
