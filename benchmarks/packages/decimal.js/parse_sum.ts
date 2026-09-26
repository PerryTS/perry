// decimal.js: parse many decimal strings and sum/compare them (ledger-style).
import Decimal from "decimal.js";
import { iters, header, lcg } from "../_lib/bench.ts";

const it = iters(2000, 100);
header("decimal.js/parse_sum", "decimal.js", it);
// Loop bounds as locals: re-reading `it.n` per iteration would add harness
// cost to the measurement (Perry: a by-name property read, ~1k instructions).
const N = it.n, WARM = it.warm;
const rnd = lcg(42);
const amounts: string[] = [];
for (let k = 0; k < 200; k++) amounts.push((rnd() % 1000000) + "." + String(rnd() % 100).padStart(2, "0"));
let last = "";
function op(i: number): void {
  let sum = new Decimal(0), max = new Decimal(-1);
  for (let k = 0; k < amounts.length; k++) {
    const d = new Decimal(amounts[(k + i) % amounts.length]);
    sum = sum.plus(d);
    if (d.gt(max)) max = d;
  }
  last = sum.toFixed(2) + "/" + max.toString() + "/" + sum.div(amounts.length).toDecimalPlaces(4).toString();
}
for (let i = 0; i < WARM; i++) op(i);
for (let i = 0; i < N; i++) op(i);
console.log("last " + last);
