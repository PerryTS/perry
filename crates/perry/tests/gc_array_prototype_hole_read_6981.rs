//! #6981 — a hole read through a polluted `Array.prototype` must terminate
//! after the prototype has RELOCATED.
//!
//! `array::indexing` memoizes `Array.prototype`'s heap address in a global
//! `AtomicUsize`. Every reader of an array pointer resolves it through
//! `clean_arr_ptr`, which follows `GC_FLAG_FORWARDED` chains; the cache did
//! not. `array_oob_prototype_get`'s self-recursion guard is the object-identity
//! test `proto != receiver`, so the moment the prototype moves — leaving a
//! forwarding stub at the memoized address — the two sides name the same object
//! by two different addresses, the guard stops firing, and
//! `js_array_get_f64` ⇄ `array_oob_prototype_get` recurse until the thread's
//! stack guard page. The OS reports `EXC_BAD_ACCESS` / `KERN_PROTECTION_FAILURE`
//! with "Thread stack size exceeded due to excessive recursion"; the process
//! dies with SIGSEGV (exit 139). It is a control-flow defect, not memory
//! corruption: every dereference on the way down is to valid, mapped memory.
//!
//! Three conditions are individually necessary, and both programs below carry
//! all three:
//!
//!   1. `Array.prototype` has RELOCATED, so the memoized address is a stub;
//!   2. an `Array.prototype` **index write** exists, which is what sets
//!      `ARRAY_PROTO_HAS_INDEX` and arms the prototype-consulting fallback
//!      (merely *naming* `Array.prototype` is not enough);
//!   3. a **hole read** — reading an index that was never assigned — which is
//!      what enters the fallback at all. Pre-filling the array removes the
//!      crash entirely.
//!
//! # Why two programs
//!
//! There are two independent ways for `Array.prototype` to relocate, and they
//! need different defences, so a single arm could pass while the other stayed
//! broken:
//!
//!   * `grow` — `Array.prototype[300] = v` runs `js_array_grow`, which
//!     reallocates the dense backing store and forwards the old head. **No GC
//!     is involved**, so no collector fix can reach it; the memoized address
//!     has to resolve the chain itself. This arm reproduces with the collector
//!     switched off entirely, which is also what proves the bug was never
//!     "a GC bug".
//!   * `relocate` — the copying young-gen minor evacuates the prototype. Here
//!     the collector must REWRITE the cache, because from-space is reset and
//!     handed back to the mutator at the end of the cycle, after which the
//!     forwarded bit is gone and chain-following cannot recover the address.
//!
//! Measured against the unfixed compiler at `c0d98624a`: `grow` exits 139 in
//! every arm including `PERRY_GEN_GC=0`; `relocate` exits 139 in every arm that
//! runs a copying minor and passes in the arms that never relocate. With the
//! fix both are node-exact everywhere.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// GROW arm. `Array.prototype[300] = 555` is past the dense capacity, so
/// `js_array_grow` reallocates and leaves a forwarding stub at the memoized
/// address. `c[1]` is then a hole read on a 4-element `new Array(4)`, which
/// consults the (relocated) prototype. Needs no GC at all.
const SOURCE_GROW: &str = r#"
(Array.prototype as any)[300] = 555;
const c: number[] = new Array(4);
c[0] = 1;
console.log("" + c[1], "" + c[300]);
delete (Array.prototype as any)[300];
"#;

const ORACLE_GROW: &str = "undefined 555\n";

/// RELOCATE arm. The prototype write is in-capacity (no grow), so the ONLY way
/// the memoized address goes stale is a relocating collection. `histogram`
/// allocates enough to trip one under a heap budget and reads `counts[v]` as a
/// hole on every first touch of a bucket.
const SOURCE_RELOCATE: &str = r#"
(Array.prototype as any)[3] = 555;
function histogram(data: number[], size: number): number[] {
  const counts: number[] = new Array(size);
  const mask = size - 1;
  for (let i = 0; i < data.length; i++) {
    const v = data[i] & mask;
    counts[v] = (counts[v] || 0) + 1;
  }
  return counts;
}
const data: number[] = [];
let seed = 4242;
for (let i = 0; i < 2000; i++) {
  seed = (seed * 48271) % 2147483647;
  data.push(seed);
}
console.log(histogram(data, 16).join(","));
"#;

/// node 26.5.0 (`.node-version`). Index 3 is the inherited 555, so its bucket
/// count is 555 higher than the others — the oracle would still be byte-exact
/// if the inherited value were dropped, which is why the assertion is on the
/// whole line and not just on termination.
const ORACLE_RELOCATE: &str = "120,105,125,679,142,125,133,128,115,135,126,109,121,117,134,141\n";

/// The test runner's own environment is inherited by `Command`, so a developer
/// (or a bisect script) exporting a collector kill switch would silently turn
/// every arm into a never-relocates control and the suite would pass against
/// the unfixed compiler. Clear the whole family, then apply the arm's own vars.
const GC_ENV_OVERRIDES: &[&str] = &[
    "PERRY_GEN_GC",
    "PERRY_GEN_GC_EVACUATE",
    "PERRY_GC_SCAVENGE",
    "PERRY_GC_SCAVENGE_NURSERY_MB",
    "PERRY_GC_MOVING_SAFEPOINT",
    "PERRY_GC_MOVING_LOOP_POLLS",
    "PERRY_GC_FORCE_EVACUATE",
    "PERRY_GC_VERIFY_EVACUATION",
    "PERRY_CONSERVATIVE_STACK_SCAN",
    "PERRY_WRITE_BARRIERS",
    "PERRY_GC_INCREMENTAL",
    "PERRY_GC_HEAP_LIMIT",
];

fn compile(dir: &std::path::Path, source: &str, name: &str) -> PathBuf {
    let entry = dir.join(format!("{name}.ts"));
    let output = dir.join(format!("{name}_bin"));
    std::fs::write(&entry, source).expect("write entry");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed for {name}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    output
}

fn run_arms(binary: &std::path::Path, dir: &std::path::Path, oracle: &str, what: &str) {
    // Runtime-only knobs, so every arm runs exactly the same generated code.
    let arms: Vec<(&str, Vec<(&str, &str)>)> = vec![
        ("default", vec![]),
        ("heap_limit_8", vec![("PERRY_GC_HEAP_LIMIT", "8")]),
        (
            "precise_roots",
            vec![
                ("PERRY_GC_HEAP_LIMIT", "8"),
                ("PERRY_GC_INCREMENTAL", "0"),
                ("PERRY_CONSERVATIVE_STACK_SCAN", "off"),
            ],
        ),
        (
            "force_evacuate",
            vec![
                ("PERRY_GC_HEAP_LIMIT", "8"),
                ("PERRY_GC_INCREMENTAL", "0"),
                ("PERRY_CONSERVATIVE_STACK_SCAN", "off"),
                ("PERRY_GC_FORCE_EVACUATE", "1"),
            ],
        ),
        // Control: full mark-sweep never runs a copying minor. The `grow` arm
        // must still fail here on the unfixed compiler (its relocation is
        // `js_array_grow`, not the collector), so a green control is not
        // inertness for that program.
        ("gen_gc_off", vec![("PERRY_GEN_GC", "0")]),
    ];

    for (label, arm) in &arms {
        let mut cmd = Command::new(binary);
        cmd.current_dir(dir);
        for key in GC_ENV_OVERRIDES {
            cmd.env_remove(key);
        }
        for (k, v) in arm {
            cmd.env(k, v);
        }
        let run = cmd.output().expect("run compiled binary");
        assert!(
            run.status.success(),
            "[{what}/{label}] compiled binary died (exit {:?}). Exit 139 here is \
             the #6981 stack overflow: the memoized `Array.prototype` address \
             went stale across a relocation, so the hole-read fallback's \
             `proto != receiver` identity guard compared a from-space address \
             against a forwarding-resolved one, never fired, and \
             `js_array_get_f64` ⇄ `array_oob_prototype_get` recursed until the \
             stack guard page.\nstderr:\n{}",
            run.status.code(),
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            oracle,
            "[{what}/{label}] output must be byte-exact vs node 26.5.0"
        );
    }
}

#[test]
fn hole_read_through_a_grown_array_prototype_terminates() {
    let dir = tempfile::tempdir().expect("tempdir");
    let binary = compile(dir.path(), SOURCE_GROW, "grow");
    run_arms(&binary, dir.path(), ORACLE_GROW, "grow");
}

#[test]
fn hole_read_through_a_relocated_array_prototype_terminates() {
    let dir = tempfile::tempdir().expect("tempdir");
    let binary = compile(dir.path(), SOURCE_RELOCATE, "relocate");
    run_arms(&binary, dir.path(), ORACLE_RELOCATE, "relocate");
}
