// uuid: deterministic v5 (SHA-1 namespace hash) + parse/stringify round trip.
import { v5, parse, stringify } from "uuid";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";

const it = iters(50000, 2000);
header("uuid/v5_parse", "uuid", it);
const NS = "6ba7b811-9dad-11d1-80b4-00c04fd430c8"; // URL namespace

function op(i: number, h: number): number {
  const u = v5("https://example.com/item/" + (i % 5000), NS);
  const bytes = parse(u);
  const back = stringify(bytes);
  return fnv(h, back + (bytes[6] >> 4));
}
let h = FNV_SEED;
for (let i = 0; i < it.warm; i++) h = op(i, h);
h = FNV_SEED;
for (let i = 0; i < it.n; i++) h = op(i, h);
console.log("checksum " + hex(h));
