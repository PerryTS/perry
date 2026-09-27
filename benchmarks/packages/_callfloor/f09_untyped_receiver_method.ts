// Call-shape floor probe (scripts/package_bench_attr.py floor): <bin> N.
const N = parseInt(process.argv[2], 10);
let acc = 0;
class C { k = 3; m(x: number): number { return (x * this.k + 1) % 1000003; } }
function drive(o: any, n: number) { let a = 0; for (let i = 0; i < n; i++) a = o.m(a + i); return a; }
acc = drive(new C(), N);
console.log("acc " + acc);
