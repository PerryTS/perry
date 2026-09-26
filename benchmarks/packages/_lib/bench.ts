// Shared protocol for every workload under benchmarks/packages/.
//
//   <arm> <workload>.ts <N> <WARM>
//
// A workload runs WARM untimed iterations, then N iterations of its hot
// operation, and prints ONLY deterministic text: the resolved package
// version and a checksum/summary of what the iterations computed. It never
// prints a timing — scripts/package_bench.py times the whole process from
// outside and derives per-iteration cost from two iteration counts
// (N1 < N2), which cancels startup, module init and warm-up.
//
// Because the output is deterministic, the runner diffs every arm's stdout
// against Node's byte-for-byte before any timing is trusted.
import * as fs from "node:fs";

export interface Iters {
  n: number;
  warm: number;
}

function intArg(pos: number, envName: string, dflt: number): number {
  const raw = process.argv[pos] ?? process.env[envName];
  if (raw === undefined || raw === "") return dflt;
  const v = parseInt(raw, 10);
  if (!(v >= 0)) throw new Error("bad iteration count: " + raw);
  return v;
}

export function iters(defaultN: number, defaultWarm: number): Iters {
  return {
    n: intArg(2, "PKG_BENCH_N", defaultN),
    warm: intArg(3, "PKG_BENCH_WARM", defaultWarm),
  };
}

// Read the INSTALLED version from node_modules (cwd is benchmarks/packages;
// the runner guarantees it). Reading the file directly works even for
// packages whose "exports" map hides ./package.json.
export function pkgVersion(name: string): string {
  const text = fs.readFileSync("node_modules/" + name + "/package.json", "utf8");
  return JSON.parse(text).version;
}

// 32-bit FNV-1a over UTF-16 code units. Deterministic across engines.
export function fnv(h: number, s: string): number {
  let x = h >>> 0;
  for (let i = 0; i < s.length; i++) {
    x ^= s.charCodeAt(i);
    x = Math.imul(x, 16777619) >>> 0;
  }
  return x;
}

export const FNV_SEED = 2166136261;

export function hex(h: number): string {
  return (h >>> 0).toString(16).padStart(8, "0");
}

// Deterministic PRNG (LCG, Numerical Recipes constants) for workload data.
export function lcg(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
    return s;
  };
}

export function header(workload: string, pkg: string, it: Iters): void {
  const v = pkg === "" ? "-" : pkgVersion(pkg);
  console.log(workload + " " + pkg + "@" + v + " n=" + it.n + " warm=" + it.warm);
}
