// node-forge: HMAC-SHA256 over short messages with a fixed key.
import forge from "node-forge";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";

const it = iters(20000, 1000);
header("node-forge/hmac", "node-forge", it);
// Loop bounds as locals: re-reading `it.n` per iteration would add harness
// cost to the measurement (Perry: a by-name property read, ~1k instructions).
const N = it.n, WARM = it.warm;
const key = "pkg-bench-hmac-key-0123456789";

function op(i: number, h: number): number {
  const hm = forge.hmac.create();
  hm.start("sha256", key);
  hm.update("GET\n/api/v1/items/" + (i % 1000) + "\nts=" + (1700000000 + i));
  return fnv(h, hm.digest().toHex());
}
let h = FNV_SEED;
for (let i = 0; i < WARM; i++) h = op(i, h);
h = FNV_SEED;
for (let i = 0; i < N; i++) h = op(i, h);
console.log("checksum " + hex(h));
