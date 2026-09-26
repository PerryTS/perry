// node-forge: SHA-256 over a ~1 KiB message per iteration.
import forge from "node-forge";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";

const it = iters(10000, 500);
header("node-forge/sha256", "node-forge", it);
const msg = "The quick brown fox jumps over the lazy dog. ".repeat(23);

function op(i: number, h: number): number {
  const md = forge.md.sha256.create();
  md.update(msg + i, "utf8");
  return fnv(h, md.digest().toHex());
}
let h = FNV_SEED;
for (let i = 0; i < it.warm; i++) h = op(i, h);
h = FNV_SEED;
for (let i = 0; i < it.n; i++) h = op(i, h);
console.log("checksum " + hex(h));
