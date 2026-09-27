// qs: stringify nested objects with arrays (brackets + indices formats).
import qs from "qs";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";

const it = iters(20000, 1000);
header("qs/stringify_nested", "qs", it);
// Loop bounds as locals: re-reading `it.n` per iteration would add harness
// cost to the measurement (Perry: a by-name property read, ~1k instructions).
const N = it.n, WARM = it.warm;

function op(i: number, h: number): number {
  const obj = {
    user: { name: "alice " + (i % 97), roles: ["admin", "dev"], meta: { a: i % 7, b: "x&y=z" } },
    filter: { age: { gte: 18, lte: 65 }, status: ["active", "pending"] },
    page: { size: 20, number: i % 50 },
  };
  const a = qs.stringify(obj, { arrayFormat: "brackets" });
  const b = qs.stringify(obj, { arrayFormat: "indices", encode: false });
  return fnv(fnv(h, a), b);
}
let h = FNV_SEED;
for (let i = 0; i < WARM; i++) h = op(i, h);
h = FNV_SEED;
for (let i = 0; i < N; i++) h = op(i, h);
console.log("checksum " + hex(h));
