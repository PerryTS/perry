// Call-shape floor probe (scripts/package_bench_attr.py floor): <bin> N.
const N = parseInt(process.argv[2], 10);
let acc = 0;
function f(this: any, x: number): number { return (x * this.k + 1) % 1000003; }
const o = { k: 3 };
for (let i = 0; i < N; i++) acc = f.call(o, acc + i);
console.log("acc " + acc);
