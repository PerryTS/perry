//! #7055 — the relocating young collection must not invalidate a closure's
//! own `this_closure` pointer.
//!
//! A closure body reaches its captured variables through
//! `js_closure_get_capture_bits(%this_closure, idx)`, where `%this_closure` is
//! the LLVM parameter the caller passed — a register value no root enumeration
//! can see. The shipped default runs an evacuating young collection at loop
//! back-edge polls (`js_gc_loop_safepoint`) with PRECISE roots and no
//! conservative native-stack scan, so a closure relocated while its own body is
//! on the stack leaves that register pointing into from-space. From-space is
//! reset at the end of the same cycle and immediately handed back to the
//! mutator, so the next capture access reads a *different* object's header:
//! `js_closure_get_capture_bits` finds the index beyond that object's
//! `capture_count` and returns **0**. Box pointer 0 is not a registered box, so
//! every later boxed-capture read yields `undefined` and every boxed-capture
//! write is silently dropped.
//!
//! In an `async fn` the casualty is the generator state machine itself. Its
//! `__gen_state` local is a boxed capture, so the `state = <next>` store at the
//! end of a resumed state body went nowhere; the following `await` then resumed
//! into the state it had just finished and **ran one loop iteration twice**.
//! That is #7055's "deterministic wrong answer in the shipped default": a
//! checksum off by exactly one request's fold, the same constant at every
//! workload length, only when `copied_objects > 0`.
//!
//! The program below is the reduced form of the issue's `w5_srv_scale.ts`.
//! Every arm must reproduce node 26.5.0 exactly:
//!
//! ```text
//! $ node --experimental-strip-types main.ts
//! calls:150
//! checksum:-342133776
//! ```
//!
//! # Why the arms are a nursery-cap sweep
//!
//! Whether a *relocating* minor lands inside the state-machine body at all
//! depends on where the nursery happens to fill, so any single configuration is
//! a coin flip on a given host — precisely the "passes either way" trap. The
//! sweep removes the coin flip: `PERRY_GC_SCAVENGE_NURSERY_MB` is a pacing knob
//! only (it moves the trigger threshold, it does not change what the collector
//! does), so one binary run under a spread of caps puts a relocating minor at
//! many different points of the program. Every arm must be node-exact; the test
//! is red if **any** of them is not.
//!
//! Measured against the unfixed compiler on this program: caps 1, 2 and 8 MB
//! print `calls:151`, caps 4 and 16 MB and the default do not. With the fix all
//! of them are node-exact. `PERRY_GEN_GC=0` (full mark-sweep, never moves) is
//! the control — correct before and after, so a green run cannot be inertness.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Reduced `w5_srv_scale.ts`: an event-loop request pump. `handle` allocates
/// enough per request to trip a relocating minor at a loop back-edge poll while
/// the async function's state-machine closure is suspended one frame up, and
/// the poll is followed by boxed-capture accesses (`__gen_state` among them).
const SOURCE: &str = r#"
let calls = 0;
let checksum = 0;

function handle(req: number): number {
  const rows: any[] = [];
  for (let i = 0; i < 1500; i++) {
    rows.push({
      id: req * 1500 + i,
      name: "row-" + i + "-" + req,
      tags: ["a" + (i & 15), "b" + (i & 7), "c" + (i % 5)],
      score: (i * 7919 + req * 104729) % 1000,
    });
  }
  let acc = 0;
  for (let i = 0; i < rows.length; i++) {
    const row = rows[i];
    acc = (acc + row.id + row.score + row.name.length + row.tags[0].length) | 0;
  }
  return acc;
}

async function main(): Promise<void> {
  for (let r = 0; r < 150; r++) {
    calls = calls + 1;
    checksum = (checksum + handle(r)) | 0;
    await new Promise<void>((resolve) => { setImmediate(resolve); });
  }
  console.log("calls:" + calls);
  console.log("checksum:" + checksum);
}

main();
"#;

/// node 26.5.0 (`.node-version`) on the program above.
const NODE_ORACLE: &str = "calls:150\nchecksum:-342133776\n";

#[test]
fn relocating_minor_does_not_replay_an_async_loop_iteration() {
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

    // One binary, many collector configurations: `PERRY_GC_SCAVENGE_NURSERY_MB`
    // and `PERRY_GEN_GC` are runtime-only knobs, so every arm runs exactly the
    // same generated code.
    let mut arms: Vec<Vec<(&str, &str)>> = vec![vec![]];
    for mb in ["1", "2", "3", "4", "5", "8", "12", "16"] {
        arms.push(vec![("PERRY_GC_SCAVENGE_NURSERY_MB", mb)]);
    }
    arms.push(vec![("PERRY_GEN_GC", "0")]);

    for arm in &arms {
        let mut cmd = Command::new(&output);
        cmd.current_dir(dir.path());
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
            "[{label}] compiled binary failed (exit {:?})\nstderr:\n{}",
            run.status.code(),
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            NODE_ORACLE,
            "[{label}] the async loop body must run exactly once per iteration \
             and fold the node-oracle checksum. `calls:151` means a relocating \
             minor invalidated the state-machine closure's own `this_closure` \
             pointer mid-body, so the `__gen_state` store was dropped and the \
             next `await` resumed into the state that had just finished (#7055)."
        );
    }
}
