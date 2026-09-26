// dayjs (+utc plugin): parse ISO strings, add, and format with tokens.
import dayjs from "dayjs";
import utc from "dayjs/plugin/utc.js";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";

dayjs.extend(utc);
const it = iters(20000, 1000);
header("dayjs/parse_format", "dayjs", it);
const inputs = ["2024-03-15T10:20:30Z", "2023-12-31T23:59:59Z", "2020-02-29T00:00:00Z", "1999-07-04T12:00:00Z"];

function op(i: number, h: number): number {
  const d = dayjs.utc(inputs[i & 3]).add(i % 400, "day").add(i % 13, "hour");
  return fnv(h, d.format("YYYY-MM-DD HH:mm:ss ddd MMM") + d.valueOf());
}
let h = FNV_SEED;
for (let i = 0; i < it.warm; i++) h = op(i, h);
h = FNV_SEED;
for (let i = 0; i < it.n; i++) h = op(i, h);
console.log("checksum " + hex(h));
