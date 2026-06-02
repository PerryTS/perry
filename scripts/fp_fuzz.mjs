#!/usr/bin/env node
// Differential floating-point fuzzer: generates randomized TS programs
// exercising reassoc / FMA-contract sensitive arithmetic patterns,
// compiles each with Perry, runs the same source under Node, and diffs
// stdout byte-for-byte. Any divergence implies the f64 bits differ
// (Number#toString is shortest-round-trip, so identical bits → identical
// strings under both engines' ryu-based formatters).
//
// Targets the patterns from issue #139's reassoc/-ffast-math discussion:
//   - reduction order (left-fold vs tree-fold vs right-fold)
//   - FMA contraction (a*b + c collapsing to one rounding step)
//   - cancellation predicates (=== 0 flips)
//   - identity round-trips (x + t - t, a*1, a/b*b)
//
// Usage:
//   node scripts/fp_fuzz.mjs                          # 50 cases, random seed
//   node scripts/fp_fuzz.mjs --count 500 --seed 42    # reproducible
//   node scripts/fp_fuzz.mjs --verbose                # per-case markers
//   node scripts/fp_fuzz.mjs --replay <fail_*.ts>     # rerun a saved case
//
// Failures are dumped under fp_fuzz_failures/ as the .ts source plus a
// .report with both stdouts.

import { spawnSync } from "node:child_process";
import { writeFileSync, mkdirSync, existsSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(__dirname, "..");

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (!a.startsWith("--")) continue;
    const key = a.slice(2);
    const next = argv[i + 1];
    if (next === undefined || next.startsWith("--")) {
      out[key] = "true";
    } else {
      out[key] = next;
      i++;
    }
  }
  return out;
}

const args = parseArgs(process.argv.slice(2));
const COUNT = parseInt(args.count ?? "50", 10);
const SEED = parseInt(args.seed ?? Math.floor(Math.random() * 0x7fffffff).toString(), 10);
const PERRY_BIN = args.perry ?? join(REPO_ROOT, "target/release/perry");
const FAIL_DIR = args.faildir ?? join(REPO_ROOT, "fp_fuzz_failures");
const VERBOSE = args.verbose === "true" || args.verbose === "1";
const REPLAY = args.replay;

// mulberry32 — small reproducible PRNG.
function mulberry32(seed) {
  let s = seed >>> 0;
  return function () {
    s = (s + 0x6d2b79f5) >>> 0;
    let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// Magnitudes spanning subnormal-adjacent through near-overflow, log-uniform
// in the exponent so we hit the precision-loss regime where (a+b)+c ≠ a+(b+c).
function randomFp(rng) {
  const r = rng();
  if (r < 0.04) {
    const specials = [
      0, -0, 1, -1, 0.5, -0.5,
      Number.EPSILON, -Number.EPSILON,
      Number.MAX_SAFE_INTEGER, -Number.MAX_SAFE_INTEGER,
      4503599627370496, // 2^52, the rintf "toint" constant
      1e-300, 1e300,
    ];
    return specials[Math.floor(rng() * specials.length)];
  }
  const sign = rng() < 0.5 ? -1 : 1;
  const expLo = -16, expHi = 16;
  const exp = expLo + (expHi - expLo) * rng();
  const mantissa = 1 + rng() * 9; // [1, 10)
  return sign * mantissa * Math.pow(10, exp);
}

// Render a number as a TS-source literal that, when read back by both
// engines' parsers, reproduces the exact same f64 bits. Plain
// Number#toString already round-trips for finite values; specials need
// keyword form.
function tsLit(x) {
  if (Object.is(x, -0)) return "-0";
  if (Number.isNaN(x)) return "NaN";
  if (x === Infinity) return "Infinity";
  if (x === -Infinity) return "-Infinity";
  return x.toString();
}

function genProgram(rng, seed, idx) {
  const N = 6;
  const ops = Array.from({ length: N }, () => randomFp(rng));
  const lits = ops.map(tsLit).join(", ");

  return `// fpfuzz seed=${seed} idx=${idx}
const xs: number[] = [${lits}];
const a = xs[0];
const b = xs[1];
const c = xs[2];
const d = xs[3];
const e = xs[4];
const f = xs[5];

// --- reduction order: left-fold vs tree-fold vs right-fold.
// reassoc lets LLVM convert one to another. Node always evaluates left-to-right.
let sumLR = 0;
for (let i = 0; i < xs.length; i++) sumLR = sumLR + xs[i];
console.log("sumLR:", sumLR.toString());

const tree = ((a + b) + (c + d)) + (e + f);
console.log("tree:", tree.toString());

const rfold = a + (b + (c + (d + (e + f))));
console.log("rfold:", rfold.toString());

// --- products: same three orderings.
let prodLR = 1;
for (let i = 0; i < xs.length; i++) prodLR = prodLR * xs[i];
console.log("prodLR:", prodLR.toString());

const prodTree = ((a * b) * (c * d)) * (e * f);
console.log("prodTree:", prodTree.toString());

// --- FMA contraction: a*b + c. With \`contract\` LLVM may fuse to one
// rounding step (fma); Node never does. The two are observably different
// when |a*b| is large enough to round before the +c is applied.
const fma1 = a * b + c;
const fma2 = c + a * b;
console.log("fma1:", fma1.toString(), "fma2:", fma2.toString());

// Long FMA chain — multiple fuse opportunities in sequence.
const chain = a * b + c * d + e * f;
console.log("chain:", chain.toString());

// --- rintf-style x + t - t. Survives reassoc alone; would collapse under nsz.
const t = 4503599627370496; // 2^52
const ident = (a + t) - t;
console.log("ident:", ident.toString());

// --- catastrophic cancellation + boolean predicate.
// (a + b) + (-a - b) is mathematically zero. Reassoc may yield exactly 0
// or a small nonzero residual depending on order; the === 0 check turns
// any difference into a control-flow flip.
const cancel = (a + b) + (-a - b);
console.log("cancel:", cancel.toString(), "isZero:", cancel === 0);

// --- multiplicative identity: a * 1.0 * 1.0. Reassoc on fmul could
// reorder these but the result is the same; sanity check.
const mulid = a * 1.0 * 1.0 * 1.0;
console.log("mulid:", mulid.toString());

// --- division round-trip: a / b * b. Reassoc may reorder; arcp would
// rewrite a/b → a*(1/b), but Perry doesn't enable arcp, so this should
// match Node bit-for-bit.
const divrt = (b !== 0 && Number.isFinite(b)) ? (a / b) * b : 0;
console.log("divrt:", divrt.toString());

// --- mixed: simulate a small dot product, the canonical FMA target.
const dot = a * d + b * e + c * f;
console.log("dot:", dot.toString());
`;
}

function runOne({ idx, src, perryBin }) {
  const tag = `fpfuzz_${process.pid}_${idx}`;
  const tsPath = join(tmpdir(), `${tag}.ts`);
  const binPath = join(tmpdir(), `${tag}.bin`);
  writeFileSync(tsPath, src);

  const node = spawnSync("node", ["--experimental-strip-types", tsPath], {
    encoding: "utf8",
    timeout: 30_000,
  });
  if (node.status !== 0) {
    return { ok: false, reason: "node-failed", src, nodeStderr: node.stderr };
  }

  const compile = spawnSync(perryBin, [tsPath, "-o", binPath], {
    encoding: "utf8",
    timeout: 120_000,
  });
  if (compile.status !== 0) {
    return { ok: false, reason: "perry-compile-failed", src, perryStderr: compile.stderr };
  }

  const perry = spawnSync(binPath, [], { encoding: "utf8", timeout: 30_000 });
  if (perry.status !== 0) {
    return { ok: false, reason: "perry-run-failed", src, perryStderr: perry.stderr };
  }

  if (node.stdout !== perry.stdout) {
    return {
      ok: false,
      reason: "output-diverged",
      src,
      nodeStdout: node.stdout,
      perryStdout: perry.stdout,
    };
  }
  return { ok: true };
}

function dumpFailure(failDir, seed, idx, result) {
  if (!existsSync(failDir)) mkdirSync(failDir, { recursive: true });
  const base = join(failDir, `fail_${seed}_${idx}`);
  writeFileSync(`${base}.ts`, result.src);
  writeFileSync(
    `${base}.report`,
    JSON.stringify(
      {
        reason: result.reason,
        nodeStdout: result.nodeStdout,
        perryStdout: result.perryStdout,
        nodeStderr: result.nodeStderr,
        perryStderr: result.perryStderr,
      },
      null,
      2,
    ),
  );
  return base;
}

function diffPreview(node, perry, maxLines = 12) {
  const nlines = node.split("\n");
  const plines = perry.split("\n");
  const out = [];
  const len = Math.max(nlines.length, plines.length);
  for (let i = 0; i < len && out.length < maxLines; i++) {
    if (nlines[i] !== plines[i]) {
      out.push(`  L${i + 1} node : ${nlines[i] ?? "<EOF>"}`);
      out.push(`  L${i + 1} perry: ${plines[i] ?? "<EOF>"}`);
    }
  }
  return out.join("\n");
}

if (!existsSync(PERRY_BIN)) {
  console.error(`Perry binary not found at ${PERRY_BIN}.`);
  console.error(`Build with: cargo build --release`);
  process.exit(2);
}

if (REPLAY) {
  const src = readFileSync(REPLAY, "utf8");
  const result = runOne({ idx: "replay", src, perryBin: PERRY_BIN });
  if (result.ok) {
    console.log(`replay: pass — outputs match`);
    process.exit(0);
  }
  console.log(`replay: ${result.reason}`);
  if (result.nodeStdout != null) {
    console.log("--- node stdout ---");
    process.stdout.write(result.nodeStdout);
    console.log("--- perry stdout ---");
    process.stdout.write(result.perryStdout);
    console.log("--- diff ---");
    console.log(diffPreview(result.nodeStdout, result.perryStdout));
  }
  if (result.nodeStderr) console.error("node stderr:", result.nodeStderr);
  if (result.perryStderr) console.error("perry stderr:", result.perryStderr);
  process.exit(1);
}

console.log(`fp_fuzz: count=${COUNT} seed=${SEED} perry=${PERRY_BIN}`);
const rng = mulberry32(SEED);
let pass = 0;
let fail = 0;
const firstFailures = [];

for (let i = 0; i < COUNT; i++) {
  const src = genProgram(rng, SEED, i);
  const result = runOne({ idx: i, src, perryBin: PERRY_BIN });

  if (result.ok) {
    pass++;
    if (VERBOSE) process.stdout.write(".");
  } else {
    fail++;
    const base = dumpFailure(FAIL_DIR, SEED, i, result);
    if (firstFailures.length < 5) firstFailures.push({ idx: i, base, ...result });
    if (VERBOSE) process.stdout.write("F");
  }
  if (!VERBOSE && (i + 1) % 10 === 0) {
    process.stdout.write(`\rprogress: ${i + 1}/${COUNT} pass=${pass} fail=${fail}`);
  }
}
if (VERBOSE) process.stdout.write("\n");
else process.stdout.write("\n");

console.log(`fp_fuzz: pass=${pass}/${COUNT} fail=${fail} seed=${SEED}`);

if (fail > 0) {
  console.log(`\nFailure cases written to ${FAIL_DIR}/`);
  console.log(`Replay any with: node scripts/fp_fuzz.mjs --replay <path>.ts\n`);
  for (const f of firstFailures) {
    console.log(`idx=${f.idx} reason=${f.reason} -> ${f.base}.ts`);
    if (f.reason === "output-diverged") {
      console.log(diffPreview(f.nodeStdout, f.perryStdout));
      console.log();
    } else if (f.perryStderr) {
      console.log(`  perry stderr: ${f.perryStderr.split("\n")[0]}`);
    } else if (f.nodeStderr) {
      console.log(`  node stderr: ${f.nodeStderr.split("\n")[0]}`);
    }
  }
  process.exit(1);
}
