// Call-shape floor probe (scripts/package_bench_attr.py floor): <bin> N.
const N = parseInt(process.argv[2], 10);
let acc = 0;
import { make } from "./lib.ts";
const o = make(N);
for (let i = 0; i < o.n; i++) acc = ((acc + i) * o.k + 1) % 1000003;
console.log("acc " + acc);
