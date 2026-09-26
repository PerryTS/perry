// Call-shape floor probe (scripts/package_bench_attr.py floor): <bin> N.
const N = parseInt(process.argv[2], 10);
let acc = 0;
class C { _k = 3; get k() { return this._k; } }
const o = new C();
for (let i = 0; i < N; i++) acc = (acc + i + o.k) % 1000003;
console.log("acc " + acc);
