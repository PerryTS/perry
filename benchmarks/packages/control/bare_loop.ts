// Bare-loop control: the same harness shape as every workload with a
// trivial hot operation. Its per-iteration cost is the floor the two-N
// method measures when the operation itself costs ~nothing.
import { iters, header, hex } from "../_lib/bench.ts";

const it = iters(1000000, 1000);
header("control/bare_loop", "", it);

function op(h: number, i: number): number {
  return Math.imul(h ^ i, 16777619) >>> 0;
}

let h = 2166136261;
for (let i = 0; i < it.warm; i++) h = op(h, i);
h = 2166136261;
for (let i = 0; i < it.n; i++) h = op(h, i);
console.log("checksum " + hex(h));
