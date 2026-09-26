// dayjs (+utc plugin): startOf/endOf/diff/isBefore arithmetic.
import dayjs from "dayjs";
import utc from "dayjs/plugin/utc.js";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";

dayjs.extend(utc);
const it = iters(20000, 1000);
header("dayjs/diff_startof", "dayjs", it);
const base = dayjs.utc("2024-01-01T00:00:00Z");

function op(i: number, h: number): number {
  const a = base.add(i % 1000, "hour");
  const b = a.add((i * 7) % 90, "day").endOf("month");
  const s = a.startOf("week");
  return fnv(h, b.diff(a, "day") + ":" + b.diff(a, "month", true).toFixed(4) + ":" +
    s.toISOString() + (a.isBefore(b) ? "<" : ">="));
}
let h = FNV_SEED;
for (let i = 0; i < it.warm; i++) h = op(i, h);
h = FNV_SEED;
for (let i = 0; i < it.n; i++) h = op(i, h);
console.log("checksum " + hex(h));
