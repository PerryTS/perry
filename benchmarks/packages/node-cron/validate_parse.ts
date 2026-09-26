// node-cron: validate + parse expressions into field sets.
import cron from "node-cron";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";

const it = iters(10000, 500);
header("node-cron/validate_parse", "node-cron", it);
const exprs = ["*/5 * * * *", "0 9 * * 1-5", "30 2 1,15 * *", "0 0 29 2 *", "15 */3 * * 0,6",
  "0 */10 8-18 * * *", "61 * * * *", "* * * * * * *"];

function op(i: number, h: number): number {
  const e = exprs[i % exprs.length];
  const ok = cron.validate(e);
  let s = e + "=" + ok;
  if (ok) {
    const f = cron.parse(e);
    s += ":" + f.minute.length + "," + f.hour.length + "," + f.dayOfMonth.length + "," + f.dayOfWeek.join("");
  }
  return fnv(h, s);
}
let h = FNV_SEED;
for (let i = 0; i < it.warm; i++) h = op(i, h);
h = FNV_SEED;
for (let i = 0; i < it.n; i++) h = op(i, h);
console.log("checksum " + hex(h));
