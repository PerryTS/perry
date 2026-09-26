// Call-shape floor probe (scripts/package_bench_attr.py floor): <bin> N.
const N = parseInt(process.argv[2], 10);
let acc = 0;
function f(x: number): number { return (arguments[0] * 3 + 1) % 1000003; }
for (let i = 0; i < N; i++) acc = f(acc + i);
console.log("acc " + acc);
