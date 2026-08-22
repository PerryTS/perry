//! #8583 root spilling — mixed statepoint / shadow-frame stacks are GC-correct.
//!
//! Root spilling lets one function keep its GC roots in a heap shadow frame
//! (the pre-#7370 lowering) while the rest of the program keeps native
//! statepoint roots, so a minified-bundle entry function whose relocation
//! fan-out would hang the optimizer stays compilable. The soundness question
//! it raises is new: a single call stack now carries BOTH kinds of frame, and
//! a moving minor must find and rewrite the live roots in each. Nothing on
//! `main` exercised that combination before this feature.
//!
//! This is a differential test with no node oracle. The same program is
//! compiled twice from identical source:
//!
//!   * `PERRY_ROOT_SPILL_RELOCATIONS=0` — spilling disabled, every function on
//!     native statepoints (the pre-#8583 lowering);
//!   * `PERRY_ROOT_SPILL_RELOCATIONS=1` — spill anything with a root and a
//!     call, so `run`/`make`/`main` take the shadow frame while the call-free
//!     accessor `leaf` stays on statepoints — a genuinely mixed stack.
//!
//! Both binaries run under every moving-collector configuration and must
//! produce byte-identical output. If the spilled frame's roots were invisible
//! to the collector, a relocating minor would leave a stale pointer and the
//! checksum would diverge (or the run would crash) in the `=1` arm only.
//!
//! The `=1` compile is also checked to have actually spilled (its stderr names
//! the shadow-frame functions), so a future change that stops spilling turns
//! this test into a tautology loudly rather than silently.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// `leaf` reads a field and makes no call: at `PERRY_ROOT_SPILL_RELOCATIONS=1`
/// its estimate is `slots × 0 = 0`, so it stays on native statepoints while its
/// callers spill. `run` holds `a`/`b`/`keep` live across allocating calls, so a
/// minor that fires inside `make` must find those roots in `run`'s shadow frame
/// and the `leaf` argument in `leaf`'s statepoint frame on the same stack.
const SOURCE: &str = r#"
function leaf(o: { v: number }): number {
  return o.v;
}

function make(i: number): { v: number } {
  return { v: i };
}

function run(): number {
  let acc = 0;
  const keep: { v: number }[] = [];
  for (let i = 0; i < 40000; i++) {
    const a = make(i);
    const b = make(i * 2);
    acc = (acc + leaf(a) + leaf(b)) | 0;
    if (i % 7 === 0) keep.push(a);
    if (keep.length > 128) keep.shift();
  }
  let s = 0;
  for (const k of keep) s = (s + leaf(k)) | 0;
  return (acc + s) | 0;
}

console.log("r:" + run());
"#;

/// Collector knobs cleared before each run so a developer's exported kill
/// switch cannot turn every arm into the never-relocates control (mirrors
/// `gc_closure_self_pointer_root_7055`).
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

fn compile(dir: &std::path::Path, spill_threshold: &str) -> (PathBuf, String) {
    let entry = dir.join("main.ts");
    let output = dir.join(format!("bin_spill_{spill_threshold}"));
    std::fs::write(&entry, SOURCE).expect("write entry");
    let out = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_ROOT_SPILL_RELOCATIONS", spill_threshold)
        .output()
        .expect("run perry compile");
    assert!(
        out.status.success(),
        "perry compile (PERRY_ROOT_SPILL_RELOCATIONS={spill_threshold}) failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    (output, String::from_utf8_lossy(&out.stderr).into_owned())
}

fn run_arms(binary: &std::path::Path, dir: &std::path::Path, label: &str) -> String {
    let mut arms: Vec<Vec<(&str, &str)>> = vec![vec![]];
    for mb in ["1", "2", "4", "8"] {
        arms.push(vec![("PERRY_GC_SCAVENGE_NURSERY_MB", mb)]);
    }
    arms.push(vec![("PERRY_GEN_GC", "0")]);

    let mut first: Option<String> = None;
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
            format!(
                "{label}/{}",
                arm.iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        };
        assert!(
            run.status.success(),
            "[{arm_label}] compiled binary failed (exit {:?})\nstderr:\n{}",
            run.status.code(),
            String::from_utf8_lossy(&run.stderr),
        );
        let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
        match &first {
            None => first = Some(stdout),
            Some(f) => assert_eq!(
                &stdout, f,
                "[{arm_label}] output differs between collector arms — a moving \
                 minor left a stale root in this configuration"
            ),
        }
    }
    first.expect("at least one arm ran")
}

#[test]
fn mixed_statepoint_and_shadow_frames_survive_a_relocating_minor() {
    let dir = tempfile::tempdir().expect("tempdir");

    // All-native reference and aggressively-spilled arm, from identical source.
    let (native_bin, _native_err) = compile(dir.path(), "0");
    let (spilled_bin, spilled_err) = compile(dir.path(), "1");

    // The spilled compile must have actually spilled, or the differential below
    // proves nothing. The report names each shadow-framed function at default
    // verbosity (#8421: the change is never silent).
    assert!(
        spilled_err.contains("keeps its") && spilled_err.contains("GC roots in a shadow frame"),
        "PERRY_ROOT_SPILL_RELOCATIONS=1 was expected to spill at least one \
         function, but the compile reported none:\nstderr:\n{spilled_err}"
    );

    let native_out = run_arms(&native_bin, dir.path(), "native");
    let spilled_out = run_arms(&spilled_bin, dir.path(), "spilled");

    assert!(
        native_out.starts_with("r:"),
        "unexpected program output: {native_out:?}"
    );
    assert_eq!(
        native_out, spilled_out,
        "mixed statepoint/shadow-frame stacks (spilled) diverged from the \
         all-statepoint build (native): a live root in a spilled frame was not \
         found or not rewritten by a moving minor (#8583)"
    );
}
