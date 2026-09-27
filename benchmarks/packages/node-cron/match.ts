// node-cron: match fixed dates against created (never started) tasks.
import cron from "node-cron";
import { iters, header, hex } from "../_lib/bench.ts";

const it = iters(50000, 2000);
header("node-cron/match", "node-cron", it);
// Loop bounds as locals: re-reading `it.n` per iteration would add harness
// cost to the measurement (Perry: a by-name property read, ~1k instructions).
const N = it.n, WARM = it.warm;
const exprs = ["*/5 * * * *", "0 9 * * 1-5", "30 2 1,15 * *", "15 */3 * * 0,6"];
const tasks = exprs.map((e) => cron.createTask(e, () => {}, { timezone: "UTC" }));
const t0 = Date.UTC(2024, 2, 15, 0, 0, 0);
let bits = 0;
function op(i: number): void {
  const d = new Date(t0 + i * 60000);
  for (let k = 0; k < tasks.length; k++) if (tasks[k].match(d)) bits = (bits * 31 + k + 1) >>> 0;
}
for (let i = 0; i < WARM; i++) op(i);
bits = 0;
for (let i = 0; i < N; i++) op(i);
for (const t of tasks) t.destroy();
console.log("checksum " + hex(bits));
