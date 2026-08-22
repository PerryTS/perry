//! #8619 — a proven-Number f64 accumulator derived from a typed-array read is
//! un-rooted (raw double, not a nanbox GC root) without corrupting the heap.
//!
//! A function that reads a spec-ABI-proven typed-array parameter and folds the
//! elements into an accumulator (`let x = arr[i] + 1.0; s = s + x`) used to keep
//! BOTH `x` and `s` in nanbox GC-root slots with a per-write root barrier, even
//! though every value is a genuine Number (a typed-array element is a Number
//! in-bounds and `undefined` — never a pointer — out of range, which `+`
//! launders into a Number). #8619 teaches the number-by-construction fixpoint
//! that a `TaPtr` view read is Number-or-`undefined`, so the accumulator drops
//! its root slot and its arithmetic becomes an inline `fadd`.
//!
//! This is a differential test with NO node oracle. The same program is compiled
//! twice from identical source:
//!
//!   * `PERRY_NUMBER_BY_CONSTRUCTION=0` — the fact is empty, so the accumulator
//!     stays a NaN-boxed GC root updated through `js_dynamic_string_or_number_add`
//!     (the pre-#8619 lowering);
//!   * unset (default) — the accumulator is proven Number by construction and
//!     kept in a raw `double` slot with an inline `fadd`.
//!
//! Both binaries run under every moving-collector configuration and MUST produce
//! byte-identical output. If the un-rooting were unsound — if the accumulator
//! could ever hold a pointer the collector no longer tracks — a relocating minor
//! would leave a stale pointer and the checksum would diverge (or the run would
//! crash) in the default arm only. The interleaved `keep` array forces nursery
//! collections while the un-rooted accumulator is live, so the collector is
//! actually exercised against the changed frame.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const SOURCE: &str = r#"
// Hot reducer over a typed-array PARAMETER: spec-ABI proves `arr` is one
// specific Float64Array, so `arr[i]` inlines and `x`/`s` are Number by
// construction under #8619. The `keep` array churns the nursery so a moving
// minor runs while the (un-rooted) accumulator is live.
function reduce(arr: Float64Array, n: number): number {
  let s = 0.0;
  const keep: number[] = [];
  for (let i = 0; i < n; i++) {
    let x = arr[i] + 1.0;
    s = s + x * 0.5;
    if ((i & 31) === 0) {
      keep.push(x);
      if (keep.length > 64) keep.shift();
    }
  }
  let t = 0.0;
  for (const k of keep) { t = t + k; }
  return s + t;
}

// Out-of-range / negative integer indices: a typed-array read is `undefined`
// there, and `undefined + 1.0` is the Number NaN — never a pointer or a string.
function edges(arr: Float64Array): number {
  let acc = 0.0;
  for (let i = -2; i < 6; i++) {
    let x = arr[i] + 1.0;
    acc = acc + (x !== x ? 100.0 : x);
  }
  return acc;
}

// A different numeric kind, folded with subtraction.
function reduceI32(arr: Int32Array, n: number): number {
  let s = 0.0;
  for (let i = 0; i < n; i++) {
    let x = arr[i] - 3.0;
    s = s + x;
  }
  return s;
}

let f = new Float64Array(256);
for (let i = 0; i < 256; i++) { f[i] = i * 0.25; }
let g = new Int32Array(256);
for (let i = 0; i < 256; i++) { g[i] = i - 128; }

let acc = 0.0;
for (let r = 0; r < 6000; r++) {
  acc = acc + reduce(f, 256) + reduceI32(g, 256) + edges(f);
}
console.log("acc:" + acc);
"#;

/// Collector knobs cleared before each run so a developer's exported kill switch
/// cannot turn every arm into the never-relocates control.
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

/// `number_by_construction`: unset = default (the #8619 un-rooting); "0" =
/// disabled (pre-#8619 rooted accumulator). Keyed into the object cache, so
/// `--no-cache` is belt-and-suspenders.
fn compile(dir: &std::path::Path, nbc: Option<&str>) -> PathBuf {
    let entry = dir.join("main.ts");
    let label = nbc.unwrap_or("default");
    let output = dir.join(format!("bin_nbc_{label}"));
    std::fs::write(&entry, SOURCE).expect("write entry");
    let mut cmd = Command::new(perry_bin());
    cmd.current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .arg("--no-auto-optimize");
    cmd.env_remove("PERRY_NUMBER_BY_CONSTRUCTION");
    if let Some(v) = nbc {
        cmd.env("PERRY_NUMBER_BY_CONSTRUCTION", v);
    }
    let out = cmd.output().expect("run perry compile");
    assert!(
        out.status.success(),
        "perry compile (PERRY_NUMBER_BY_CONSTRUCTION={label}) failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    output
}

fn run_arms(binary: &std::path::Path, dir: &std::path::Path, label: &str) -> String {
    let mut arms: Vec<Vec<(&str, &str)>> = vec![vec![]];
    for mb in ["1", "2", "4"] {
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
fn ta_view_accumulator_unroot_is_gc_correct() {
    let dir = tempfile::tempdir().expect("tempdir");

    // Rooted reference (fact disabled) and #8619 un-rooted arm, identical source.
    let rooted_bin = compile(dir.path(), Some("0"));
    let unrooted_bin = compile(dir.path(), None);

    let rooted_out = run_arms(&rooted_bin, dir.path(), "rooted");
    let unrooted_out = run_arms(&unrooted_bin, dir.path(), "unrooted");

    assert!(
        rooted_out.starts_with("acc:"),
        "unexpected program output: {rooted_out:?}"
    );
    assert_eq!(
        rooted_out, unrooted_out,
        "un-rooting the typed-array-derived accumulator (#8619) changed observable \
         output vs the rooted build — an un-rooted slot that can hold a pointer, or \
         a semantic divergence in the arithmetic fast path"
    );
}
