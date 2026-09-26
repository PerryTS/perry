// Call-shape floor probe (scripts/package_bench_attr.py floor): <bin> N.
const N = parseInt(process.argv[2], 10);
let acc = 0;
function f(x: number): number { if (x < 0) return f(x + 1) + 1; return (x * 3 + 1) % 1000003; }
for (let i = 0; i < N; i++) acc = f(acc + i);
console.log("acc " + acc);
