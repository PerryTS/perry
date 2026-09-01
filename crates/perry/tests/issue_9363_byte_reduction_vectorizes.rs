//! #9363 (B): a bounded byte-read reduction reassociates, and module init
//! prunes the redundant shadow root slots that were blocking it.
//!
//! Two changes that are each worth NOTHING alone and 2.8x together — which is
//! why they ship as one commit and are pinned by one test.
//!
//! 1. **`fadd reassoc` on a proven reduction.** `acc = acc + <byte read>` in a
//!    trip-count-bounded loop keeps every partial sum below 2^53, where f64
//!    addition is exact and therefore associative, so any grouping is
//!    bit-identical. An out-of-range read yields `undefined` -> NaN, which
//!    propagates through every grouping alike, so the OOB case needs no
//!    separate argument. Without the flag LLVM cannot split the serial fadd
//!    dependency chain and the loop runs at fadd latency (~3 cycles/element).
//!
//! 2. **Module-init shadow-slot pruning.** `codegen/function.rs` drops root
//!    slots for locals the whole-write proof shows can only hold a Number;
//!    module init never did. `local_is_inert_primitive` refuses any local
//!    that HAS a slot, so a proven-numeric top-level accumulator was not
//!    inert, `loop_may_allocate` stayed true, and the loop kept a
//!    per-iteration `load volatile @PERRY_GC_POLL_ARMED` — which blocks
//!    vectorization outright. This was the entire reason the same loop was
//!    fast inside a function and slow at top level.
//!
//! Measured on `bench_buffer_readwrite` (quiet host, min-of-3): 94 -> 34 ms
//! against node's 81. Reassoc alone at top level: 94 -> 94. The in-function
//! loop, already poll-free, isolates reassoc's own contribution: 94 -> 32.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// A top-level byte-sum reduction — the `bench_buffer_readwrite` shape.
/// `N` is small so the test is fast; the emitted shape is what matters.
const SOURCE: &str = r#"
const N = 4096;
const buf = new Uint8Array(N);
for (let i = 0; i < N; i++) buf[i] = i % 256;

let checksum = 0;
for (let iter = 0; iter < 3; iter++) {
    let sum = 0;
    for (let i = 0; i < N; i++) {
        sum += buf[i];
    }
    checksum += sum;
}
console.log("checksum:" + checksum);
"#;

const EXPECTED: &str = "checksum:1566720\n";

fn compile(dir: &Path, source: &str) -> (PathBuf, String) {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_LLVM_KEEP_IR", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    (
        output,
        String::from_utf8_lossy(&compile.stderr).into_owned(),
    )
}

fn kept_ir(stderr: &str) -> String {
    let path = stderr
        .lines()
        .find_map(|line| line.split("kept LLVM IR: ").nth(1))
        .map(str::trim)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("PERRY_LLVM_KEEP_IR did not report an IR path\n{stderr}"));
    std::fs::read_to_string(path).expect("read kept LLVM IR")
}

fn run(bin: &Path, dir: &Path, gc_stress: bool) -> Output {
    let mut command = Command::new(bin);
    command.current_dir(dir);
    if gc_stress {
        command
            .env("PERRY_GC_HEAP_LIMIT", "8")
            .env("PERRY_GC_FORCE_EVACUATE", "1");
    }
    command.output().expect("run compiled binary")
}

/// Both halves are present in the emitted top-level loop, and the program is
/// still correct — including under forced evacuation, which is the arm that
/// would catch a root slot pruned when it was actually needed.
#[test]
fn top_level_byte_reduction_reassociates_and_drops_its_poll() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, stderr) = compile(dir.path(), SOURCE);
    let ir = kept_ir(&stderr);

    assert!(
        ir.contains("fadd reassoc double"),
        "the proven byte reduction must carry `reassoc`; without it LLVM \
         cannot break the serial fadd dependency chain"
    );

    // The poll count is the load-bearing half of the pruning fix. It is not
    // asserted as zero for the whole module — the fill loop above writes
    // through `buf[i] = ...` and other constructs may legitimately keep one —
    // but the READ loop's block must not carry one. Locate the reassoc'd add
    // and require no volatile poll load between it and its block's terminator.
    let reassoc_line = ir
        .lines()
        .position(|l| l.contains("fadd reassoc double"))
        .expect("reassoc add present");
    let tail: Vec<&str> = ir.lines().skip(reassoc_line).take(8).collect();
    assert!(
        !tail.iter().any(|l| l.contains("PERRY_GC_POLL_ARMED")),
        "the reduction loop still polls the GC every iteration, which blocks \
         vectorization — module-init shadow-slot pruning is not firing:\n{}",
        tail.join("\n")
    );

    for stress in [false, true] {
        let out = run(&bin, dir.path(), stress);
        assert!(
            out.status.success(),
            "binary failed (gc_stress={stress})\nstderr:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            EXPECTED,
            "wrong sum (gc_stress={stress})"
        );
    }
}

/// The reassociation admission is a MAGNITUDE proof, so an accumulator whose
/// bound cannot be established must not get the flag. Here the addend is an
/// unbounded parameter rather than a byte read, so no bound exists.
#[test]
fn unbounded_accumulator_does_not_reassociate() {
    const UNBOUNDED: &str = r#"
function addAll(xs: number[]): number {
    let acc = 0;
    for (let i = 0; i < xs.length; i++) acc += xs[i];
    return acc;
}
console.log("r:" + addAll([1.5, 2.25, 3.125]));
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, stderr) = compile(dir.path(), UNBOUNDED);
    let ir = kept_ir(&stderr);
    assert!(
        !ir.contains("fadd reassoc double"),
        "an accumulator over arbitrary f64 array elements has no magnitude \
         bound, so reassociation is NOT exact for it and must not be emitted"
    );
    let out = run(&bin, dir.path(), false);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "r:6.875\n");
}

/// A pointer-valued module-scope local must keep its shadow root slot: the
/// pruning is gated on the Number-by-construction proof, and dropping a slot
/// a real pointer needs would let the collector free a live object. Forced
/// evacuation is the arm that catches it.
#[test]
fn pointer_module_local_keeps_its_root_slot() {
    const POINTERS: &str = r#"
const buf = new Uint8Array(64);
for (let i = 0; i < 64; i++) buf[i] = i;

let sum = 0;
for (let i = 0; i < 64; i++) sum += buf[i];

let holder: string[] = ["seed"];
let churn = 0;
for (let i = 0; i < 20000; i++) {
    const garbage = { a: i, b: "x" + (i % 7) };
    churn += garbage.a & 1;
    if (i % 5000 === 0) holder.push("k" + i);
}
console.log("sum:" + sum + " holder:" + holder.join(",") + " churn:" + churn);
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, _stderr) = compile(dir.path(), POINTERS);
    let expected = "sum:2016 holder:seed,k0,k5000,k10000,k15000 churn:10000\n";
    for stress in [false, true] {
        let out = run(&bin, dir.path(), stress);
        assert!(out.status.success(), "binary failed (gc_stress={stress})");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            expected,
            "a live pointer local was lost (gc_stress={stress}) — the shadow \
             pruning dropped a root slot that was actually needed"
        );
    }
}
