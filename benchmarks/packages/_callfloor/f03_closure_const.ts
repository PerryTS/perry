// Call-shape floor probe (scripts/package_bench_attr.py floor): <bin> N.
const N = parseInt(process.argv[2], 10);
let acc = 0;
function mk(k: number) { return (x: number) => (x * k + 1) % 1000003; }
const g = mk(3);
for (let i = 0; i < N; i++) acc = g(acc + i);
console.log("acc " + acc);
