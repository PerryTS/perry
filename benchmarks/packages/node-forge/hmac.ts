// node-forge: HMAC-SHA256 over short messages with a fixed key.
import forge from "node-forge";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";

const it = iters(20000, 1000);
header("node-forge/hmac", "node-forge", it);
const key = "pkg-bench-hmac-key-0123456789";

function op(i: number, h: number): number {
  const hm = forge.hmac.create();
  hm.start("sha256", key);
  hm.update("GET\n/api/v1/items/" + (i % 1000) + "\nts=" + (1700000000 + i));
  return fnv(h, hm.digest().toHex());
}
let h = FNV_SEED;
for (let i = 0; i < it.warm; i++) h = op(i, h);
h = FNV_SEED;
for (let i = 0; i < it.n; i++) h = op(i, h);
console.log("checksum " + hex(h));
