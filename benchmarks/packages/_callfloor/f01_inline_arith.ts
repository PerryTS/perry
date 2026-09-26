// Call-shape floor probe (scripts/package_bench_attr.py floor): <bin> N.
const N = parseInt(process.argv[2], 10);
let acc = 0;
for (let i = 0; i < N; i++) acc = ((acc + i) * 3 + 1) % 1000003;
console.log("acc " + acc);
