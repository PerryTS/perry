// Call-shape floor probe (scripts/package_bench_attr.py floor): <bin> N.
const N = parseInt(process.argv[2], 10);
let acc = 0;
function P(this: any) { this.k = 3; }
P.prototype.m = function (x: number): number { return (x * this.k + 1) % 1000003; };
const o = new (P as any)();
for (let i = 0; i < N; i++) acc = o.m(acc + i);
console.log("acc " + acc);
