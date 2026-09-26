// big.js: arithmetic chain with DP=30 (plus/times/pow/div/sqrt/toFixed).
import Big from "big.js";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";

Big.DP = 30;
const it = iters(20000, 1000);
header("big.js/arith_chain", "big.js", it);

function op(i: number, h: number): number {
  const principal = new Big(1000 + (i % 997)).plus("0.37");
  const rate = new Big(i % 17).div(1200);
  const amt = principal.times(rate.plus(1).pow(12 + (i % 24)));
  const r = amt.div(7).sqrt().minus(principal.div(3)).toFixed(20);
  return fnv(h, r + amt.toPrecision(12));
}
let h = FNV_SEED;
for (let i = 0; i < it.warm; i++) h = op(i, h);
h = FNV_SEED;
for (let i = 0; i < it.n; i++) h = op(i, h);
console.log("checksum " + hex(h));
