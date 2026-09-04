//! #9587 — `new Promise(executor)` must return the LIVE promise when the
//! executor allocates.
//!
//! The executor is arbitrary user JS and it runs while `js_promise_new_with_executor`
//! still owns the promise it is about to return. Claude Code's dialog helper is
//! the shape that exposed it:
//!
//! ```js
//! function qA8(root, ui) {
//!   return new Promise((res) => { let z = (y) => void res(y); root.render(ui(z)) })
//! }
//! ```
//!
//! — a whole ink/React render (megabytes of allocation) inside the executor. The
//! evacuating young collection that lands there MOVES the Promise. The resolving
//! closures survive correctly (their capture slots are GC slots, so the collector
//! rewrites them), but pre-fix `promise` lived in a bare Rust local across
//! `js_closure_call2`, so the function returned the PRE-COLLECTION address.
//! From-space is reset and handed back to the mutator at the end of the same
//! cycle, so that pointer names recycled memory. `await`ing it goes one of two
//! ways, both observed on the compiled cc 2.1.112 binary:
//!
//! * the recycled header decodes as `Fulfilled`, so the `await` never suspends
//!   and resumes immediately with a garbage value — cc advanced past an
//!   onboarding dialog nobody had answered; or
//! * it still decodes as `Pending`, so the async step parks its continuation on
//!   the dead copy. `resolve()` then settles the LIVE promise, which has no
//!   reaction, and nothing ever resumes — a silent permanent hang with no throw
//!   and no rejection. That is cc's trust dialog wedging 100% of the time on a
//!   fresh HOME (#9587), which is also why onboarding never wrote `theme` /
//!   `hasCompletedOnboarding` (#9674).
//!
//! Neither failure raises anything: `PERRY_REJECTION_DIAG` is silent and the
//! whole-heap `PERRY_GC_FROMSPACE_SCAN` reports CLEAN, because at the moment of
//! the collection the stale address exists only in a Rust register, and by the
//! time it reaches JS from-space has already been flipped. The instrument that
//! does see it is `PERRY_GC_PROTECT_FROMSPACE=1`, which faults on the stale read
//! with `obj_type=5` (`GC_TYPE_PROMISE`).
//!
//! # Why the arms are a nursery-cap sweep
//!
//! Whether a *relocating* minor lands inside the executor at all depends on where
//! the nursery happens to fill, so a single configuration is a coin flip on a
//! given host. `PERRY_GC_SCAVENGE_NURSERY_MB` is a pacing knob only — it moves
//! the trigger threshold, it does not change what the collector does — so one
//! binary under a spread of caps puts a relocating minor at many different points
//! of the executor. `PERRY_GEN_GC=0` (full mark-sweep, never moves) is the
//! control: correct before and after the fix, so a green run cannot be inertness.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// `test-files/test_issue_9587_promise_executor_evacuation.ts`, inlined so the
/// test is self-contained.
const SOURCE: &str = r#"
let saved: (() => void) | null = null;
const order: string[] = [];

function dialog(): Promise<string> {
  return new Promise<string>((resolve) => {
    saved = () => resolve("resolved");
    let sink: any = null;
    for (let i = 0; i < 200000; i++) {
      sink = { a: i, b: [i, i + 1], c: "row" + i, d: { e: i } };
    }
    if (sink === null) console.log("unreachable");
  });
}

let settled = false;

async function main(): Promise<void> {
  const p = dialog();
  order.push("created");
  p.then(() => { settled = true; });
  setTimeout(() => {
    order.push("resolving");
    console.log("settled-before-resolve:" + settled);
    (saved as () => void)();
  }, 10);
  const v = await p;
  let vs = "unreadable";
  try {
    vs = (v as unknown) === "resolved" ? "resolved" : "other";
  } catch {
    vs = "threw";
  }
  order.push("value:" + vs);
  console.log("order:" + order.join(","));
}

main();
"#;

/// node 26.5.0 (`.node-version`) on the program above.
const NODE_ORACLE: &str = "settled-before-resolve:false\norder:created,resolving,value:resolved\n";

/// The collector kill switches an ambient environment could be carrying. Cleared
/// per arm so an exported `PERRY_GEN_GC=0` cannot silently turn every arm into
/// the never-relocates control and pass against the unfixed runtime.
const GC_ENV_OVERRIDES: &[&str] = &[
    "PERRY_GEN_GC",
    "PERRY_GC_SCAVENGE",
    "PERRY_GC_SCAVENGE_NURSERY_MB",
    "PERRY_GC_MOVING_SAFEPOINT",
    "PERRY_GC_MOVING_LOOP_POLLS",
    "PERRY_GC_FORCE_EVACUATE",
    "PERRY_GC_SCHEDULE_SEED",
    "PERRY_GC_SCHEDULE_RATE",
    "PERRY_CONSERVATIVE_STACK_SCAN",
    "PERRY_WRITE_BARRIERS",
    "PERRY_GC_INCREMENTAL",
    "PERRY_GC_HEAP_LIMIT",
];

#[test]
fn promise_returned_from_an_allocating_executor_survives_an_evacuating_minor() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, SOURCE).expect("write entry");

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

    // One binary, many collector configurations: every knob below is
    // runtime-only, so all arms run exactly the same generated code.
    let mut arms: Vec<Vec<(&str, &str)>> = vec![vec![]];
    for mb in ["1", "2", "3", "4", "8", "16"] {
        arms.push(vec![("PERRY_GC_SCAVENGE_NURSERY_MB", mb)]);
    }
    // Force a collection at (nearly) every handled safepoint, with survivors
    // moving — the densest placement of a relocating minor inside the executor.
    arms.push(vec![
        ("PERRY_GC_SCHEDULE_SEED", "1"),
        ("PERRY_GC_SCHEDULE_RATE", "1"),
    ]);
    // Control: full mark-sweep never relocates, so this arm is green before and
    // after the fix.
    arms.push(vec![("PERRY_GEN_GC", "0")]);

    for arm in &arms {
        let mut cmd = Command::new(&output);
        cmd.current_dir(dir.path());
        for key in GC_ENV_OVERRIDES {
            cmd.env_remove(key);
        }
        for (k, v) in arm {
            cmd.env(k, v);
        }
        let run = cmd.output().expect("run compiled binary");
        let label = if arm.is_empty() {
            "default".to_string()
        } else {
            arm.iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(" ")
        };
        assert!(
            run.status.success(),
            "[{label}] compiled binary failed (exit {:?})\nstdout:\n{}\nstderr:\n{}",
            run.status.code(),
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            NODE_ORACLE,
            "[{label}] `new Promise(executor)` must return the promise the \
             collector kept, not the address it had before the executor \
             allocated. Output missing the `order:` line entirely means the \
             await parked on the dead copy and never resumed; an `order:` line \
             printed BEFORE `settled-before-resolve` (or carrying \
             `value:other`) means the await fell through on a recycled header \
             that decoded as Fulfilled (#9587)."
        );
    }
}
