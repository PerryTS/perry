// Bare-loop control (minimal): the same harness shape as every workload with a
// trivial hot operation (one FNV multiply step, the same helper every
// workload's checksum uses). Its per-iteration cost is the floor the two-N
// method measures when the operation itself costs ~nothing.
import { iters, header, hex, mulFnv } from "../_lib/bench.ts";

const it = iters(1000000, 1000);
header("control/bare_loop", "", it);
// Loop bounds as locals: re-reading `it.n` per iteration would add harness
// cost to the measurement (Perry: a by-name property read, ~1k instructions).
const N = it.n, WARM = it.warm;

function op(h: number, i: number): number {
  return mulFnv((h ^ i) >>> 0);
}

let h = 2166136261;
for (let i = 0; i < WARM; i++) h = op(h, i);
h = 2166136261;
for (let i = 0; i < N; i++) h = op(h, i);
console.log("checksum " + hex(h));
