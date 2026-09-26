// cron: parse a cron expression (CronTime) and compute the next fire time
// from a FIXED start instant in UTC (never "now", so output is stable).
import { CronTime } from "cron";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";

const it = iters(5000, 200);
header("cron/next_dates", "cron", it);
const exprs = ["*/5 * * * *", "0 9 * * 1-5", "30 2 1,15 * *", "0 0 29 2 *", "15 */3 * * 0,6", "0 */10 8-18 * * *"];
const start0 = Date.UTC(2024, 2, 15, 10, 20, 30);

function op(i: number, h: number): number {
  const ct = new CronTime(exprs[i % exprs.length], "UTC");
  const next = ct.getNextDateFrom(new Date(start0 + (i % 1000) * 3600000), "UTC");
  return fnv(h, next.toISO() ?? "null");
}
let h = FNV_SEED;
for (let i = 0; i < it.warm; i++) h = op(i, h);
h = FNV_SEED;
for (let i = 0; i < it.n; i++) h = op(i, h);
console.log("checksum " + hex(h));
