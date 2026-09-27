// Call-shape floor probe (scripts/package_bench_attr.py floor): <bin> N.
const N = parseInt(process.argv[2], 10);
let acc = 0;
class C { #k = 3; #m(x: number): number { return (x * this.#k + 1) % 1000003; } run(x: number): number { return this.#m(x); } }
const o = new C();
for (let i = 0; i < N; i++) acc = o.run(acc + i);
console.log("acc " + acc);
