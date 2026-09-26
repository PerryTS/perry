// date-fns: parseISO + addDays/addMonths + format. Runs with TZ=UTC (the
// runner sets it for every arm), since date-fns formats in local time.
import { parseISO, addDays, addMonths, format } from "date-fns";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";

const it = iters(20000, 1000);
header("date-fns/format_add", "date-fns", it);
const inputs = ["2024-03-15T10:20:30Z", "2023-12-31T23:59:59Z", "2020-02-29T00:00:00Z", "1999-07-04T12:00:00Z"];

function op(i: number, h: number): number {
  const d = addMonths(addDays(parseISO(inputs[i & 3]), i % 400), i % 5);
  return fnv(h, format(d, "yyyy-MM-dd HH:mm:ss EEE MMM do") + d.getTime());
}
let h = FNV_SEED;
for (let i = 0; i < it.warm; i++) h = op(i, h);
h = FNV_SEED;
for (let i = 0; i < it.n; i++) h = op(i, h);
console.log("checksum " + hex(h));
