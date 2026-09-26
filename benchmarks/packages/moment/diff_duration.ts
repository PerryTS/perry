// moment: diff, startOf/endOf, durations and ISO output.
import moment from "moment";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";

const it = iters(10000, 500);
header("moment/diff_duration", "moment", it);
// Loop bounds as locals: re-reading `it.n` per iteration would add harness
// cost to the measurement (Perry: a by-name property read, ~1k instructions).
const N = it.n, WARM = it.warm;
const base = moment.utc("2024-01-01T00:00:00Z");

function op(i: number, h: number): number {
  const a = base.clone().add(i % 1000, "hours");
  const b = a.clone().add((i * 7) % 90, "days").endOf("month");
  const d = moment.duration(b.diff(a));
  return fnv(h, b.diff(a, "days") + ":" + d.asHours().toFixed(3) + ":" + d.toISOString() + ":" +
    a.clone().startOf("isoWeek").toISOString());
}
let h = FNV_SEED;
for (let i = 0; i < WARM; i++) h = op(i, h);
h = FNV_SEED;
for (let i = 0; i < N; i++) h = op(i, h);
console.log("checksum " + hex(h));
