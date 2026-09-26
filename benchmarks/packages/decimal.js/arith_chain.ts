// decimal.js: compound-interest style arithmetic chain at 40 significant
// digits (plus/times/pow/div/sqrt/toFixed).
import Decimal from "decimal.js";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";

Decimal.set({ precision: 40, rounding: Decimal.ROUND_HALF_EVEN });
const it = iters(20000, 1000);
header("decimal.js/arith_chain", "decimal.js", it);
// Loop bounds as locals: re-reading `it.n` per iteration would add harness
// cost to the measurement (Perry: a by-name property read, ~1k instructions).
const N = it.n, WARM = it.warm;

function op(i: number, h: number): number {
  const principal = new Decimal(1000 + (i % 997)).plus("0.37");
  const rate = new Decimal(i % 17).div(1200);
  const amt = principal.times(rate.plus(1).pow(12 + (i % 24)));
  const r = amt.div(7).sqrt().minus(principal.div(3)).toFixed(20);
  return fnv(h, r + amt.toSignificantDigits(12).toString());
}
let h = FNV_SEED;
for (let i = 0; i < WARM; i++) h = op(i, h);
h = FNV_SEED;
for (let i = 0; i < N; i++) h = op(i, h);
console.log("checksum " + hex(h));
