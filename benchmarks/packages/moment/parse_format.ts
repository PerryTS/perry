// moment: strict format-string parse (moment.utc(str, fmt)), add, format.
import moment from "moment";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";

const it = iters(10000, 500);
header("moment/parse_format", "moment", it);
// Loop bounds as locals: re-reading `it.n` per iteration would add harness
// cost to the measurement (Perry: a by-name property read, ~1k instructions).
const N = it.n, WARM = it.warm;
const inputs = ["15/03/2024 10:20", "31/12/2023 23:59", "29/02/2020 00:00", "04/07/1999 12:00"];

function op(i: number, h: number): number {
  const m = moment.utc(inputs[i & 3], "DD/MM/YYYY HH:mm", true).add(i % 400, "days").add(i % 13, "hours");
  return fnv(h, m.format("dddd, MMMM Do YYYY, h:mm:ss a") + m.valueOf() + m.isValid());
}
let h = FNV_SEED;
for (let i = 0; i < WARM; i++) h = op(i, h);
h = FNV_SEED;
for (let i = 0; i < N; i++) h = op(i, h);
console.log("checksum " + hex(h));
