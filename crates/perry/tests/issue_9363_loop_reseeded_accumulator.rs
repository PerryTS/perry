//! #9363/#6898: an in-loop additive write is i32-admissible when the loop body
//! RE-SEEDS the local unconditionally on every iteration.
//!
//! `collectors/int_valued_ta_locals.rs` exists for bcryptjs `_encipher` — its
//! module doc is that function — but rejected its own subject's accumulator.
//! The wrap-i32 additive arm was restricted to STRAIGHT-LINE (never in-loop)
//! trees, and `n += S[...]` sits in the Feistel `while`. So `n` stayed an f64
//! slot holding nothing but int32 values, and every S-box step emitted
//! `sitofp` in and `llvm.aarch64.fjcvtzs` out around the `fadd`.
//!
//! ## Why the restriction was too coarse
//!
//! Its stated hazard is real: an unbounded in-loop chain can carry the true
//! f64 value past 2^53, where it ROUNDS while an i32 slot WRAPS, and rule (2)
//! only guarantees the `ToInt32` image is observed — so the two would then
//! disagree.
//!
//! A per-iteration re-seed removes exactly that hazard, and needs no dominance
//! argument: if the body unconditionally assigns the local a fresh exact-i32
//! value once per iteration, the chain restarts every iteration no matter
//! WHERE the re-seed sits, so the magnitude never exceeds one body's worth of
//! addends. With each addend below 2^31, a body would need ~4M additive writes
//! to reach 2^53. `_encipher` re-seeds `n` every round and adds at most twice:
//! `|n| < 2^33`.
//!
//! The re-seed must be at the body's TOP level. One nested in an `if` may not
//! run on a given iteration — precisely the case where the chain keeps
//! growing — and an inner loop's re-seed does not bound the OUTER body's
//! chain. Both are pinned below, because they are the shapes that would ship
//! wrapped arithmetic if the rule were widened carelessly.
//!
//! Measured on `bench_typed_array_untyped_access`: typed 1216 -> 257 ms and
//! untyped 1254 -> 258 against node's 290 / 299 — from 4.2x slower to faster
//! than node on both paths.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile(dir: &Path, source: &str) -> PathBuf {
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
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    output
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

/// Every shape whose additive chain is NOT bounded by an unconditional
/// per-iteration re-seed must keep its f64 representation. Node is the oracle
/// rather than a hand-written expectation, because the whole question is
/// whether perry's value agrees with JavaScript's.
///
/// The elements are `2^30`, so four of them exceed `i32::MAX`: a local wrongly
/// promoted to an i32 slot prints a WRAPPED negative here, not a slightly
/// different number. Cases A and D deliberately produce values above i32 range
/// (and A above 2^53's neighbourhood) so the failure would be unmissable.
#[test]
fn only_unconditionally_reseeded_accumulators_are_admitted() {
    const SOURCE: &str = r#"
const S = new Int32Array(8);
for (let i = 0; i < 8; i++) S[i] = 0x40000000;

// A: no reseed at all — accumulates across 1e6 iterations.
function noReseed(): number {
  let n = S[0];
  for (let i = 0; i < 1000000; i++) { n += S[i & 7]; }
  return n;
}
// B: reseed guarded by an `if` — does not run every iteration.
function condReseed(): number {
  let n = S[0];
  for (let i = 0; i < 1000; i++) {
    if (i === 500) { n = S[1]; }
    n += S[i & 7];
  }
  return n;
}
// C: unconditional top-level reseed — the admissible shape.
function goodReseed(): number {
  let n = 0; let acc = 0;
  for (let i = 0; i < 1000; i++) { n = S[i & 7]; n += S[(i + 1) & 7]; acc ^= n; }
  return acc;
}
// D: reseed only in an INNER loop — the outer chain is still unbounded.
function innerOnly(): number {
  let n = S[0];
  for (let i = 0; i < 1000; i++) {
    for (let j = 0; j < 2; j++) { n = S[j]; }
    n += S[i & 7];
  }
  return n;
}
// E: out-of-range reads mixed in — `undefined` must behave as JS says.
function withOob(): string {
  let n = S[0];
  let out = "";
  for (let i = 0; i < 4; i++) { n = S[i + 6]; n += S[i]; out += String(n) + ";"; }
  return out;
}
console.log("A:" + noReseed());
console.log("B:" + condReseed());
console.log("C:" + goodReseed());
console.log("D:" + innerOnly());
console.log("E:" + withOob());
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = compile(dir.path(), SOURCE);

    let node = Command::new("node")
        .current_dir(dir.path())
        .arg("--experimental-strip-types")
        .arg(dir.path().join("main.ts"))
        .output()
        .expect("run node");
    assert!(
        node.status.success(),
        "node failed on the oracle fixture:\n{}",
        String::from_utf8_lossy(&node.stderr)
    );
    // Guard the oracle itself: if these stopped exceeding i32 range the test
    // would still pass while having lost its ability to detect a wrap.
    let expected = String::from_utf8_lossy(&node.stdout).into_owned();
    assert!(
        expected.contains("A:1073742897741824") && expected.contains("D:2147483648"),
        "the oracle no longer produces values outside i32 range, so a wrongly \
         admitted local would no longer be detectable here:\n{expected}"
    );

    for stress in [false, true] {
        let out = run(&bin, dir.path(), stress);
        assert!(out.status.success(), "binary failed (gc_stress={stress})");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            expected,
            "an accumulator was promoted to an i32 slot without a bounded \
             chain (gc_stress={stress}) — the value wrapped"
        );
    }
}

/// The motivating shape itself: a Feistel round that re-seeds its accumulator
/// and adds twice before a bitwise write. Values must match node exactly.
#[test]
fn feistel_round_accumulator_matches_node() {
    const SOURCE: &str = r#"
const P = new Int32Array(18);
const S = new Int32Array(1024);
for (let i = 0; i < P.length; i++) P[i] = (i * 40503 + 7) | 0;
for (let i = 0; i < S.length; i++) S[i] = (i * 2654435761) | 0;

function encipher(lr: number[], off: number, P: Int32Array, S: Int32Array): void {
  let n: number;
  let l = lr[off];
  let r = lr[off + 1];
  l ^= P[0];
  let i = 0;
  while (i < 16) {
    n = S[l >>> 24];
    n += S[0x100 | ((l >> 16) & 0xff)];
    n ^= S[0x200 | ((l >> 8) & 0xff)];
    n += S[0x300 | (l & 0xff)];
    r ^= n ^ P[++i];
    n = S[r >>> 24];
    n += S[0x100 | ((r >> 16) & 0xff)];
    n ^= S[0x200 | ((r >> 8) & 0xff)];
    n += S[0x300 | (r & 0xff)];
    l ^= n ^ P[++i];
  }
  lr[off] = r ^ P[17];
  lr[off + 1] = l;
}

const lr = [0x01234567, 0x89abcdef];
for (let c = 0; c < 64; c++) encipher(lr, 0, P, S);
console.log(lr[0] + "," + lr[1]);
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = compile(dir.path(), SOURCE);
    let node = Command::new("node")
        .current_dir(dir.path())
        .arg("--experimental-strip-types")
        .arg(dir.path().join("main.ts"))
        .output()
        .expect("run node");
    assert!(node.status.success(), "node failed");

    for stress in [false, true] {
        let out = run(&bin, dir.path(), stress);
        assert!(out.status.success(), "binary failed (gc_stress={stress})");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&node.stdout),
            "Feistel state diverged from node (gc_stress={stress})"
        );
    }
}
