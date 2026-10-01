// Correctness across module init orders for class key lists. Module A declares
// classes whose fields `qzonly`/`qzr0`/`qzr9` are named by no literal of A;
// module B (which inits after A) and the two reader modules name them. The
// entry module names `qzmain`. Every read, own-key listing and key add must
// answer the same whichever module registered the class first.
// This fixture checks behavior only; the runtime test
// `a_class_registered_before_the_pools_answers_the_megamorphic_confirm` is the
// proof that the class key list holds the pool atoms.
import { rd_r0 } from "./_helpers/class_keys_atoms_init_order_r0.ts";
import { makeAll } from "./_helpers/class_keys_atoms_init_order_a.ts";
import { fill, rd, run } from "./_helpers/class_keys_atoms_init_order_b.ts";
import { rd_r9 } from "./_helpers/class_keys_atoms_init_order_r9.ts";

// `qzmain`: declared in A, named only here (the entry module inits last).
function rdMain(o: any): number {
  return o.qzmain;
}
function runMain(n: number, xs: any[]): number {
  let h = 0;
  for (let k = 0; k < n; k++) h += rdMain(xs[k & 15]);
  return h;
}
const N = typeof process !== "undefined" && process.argv[2] ? Number(process.argv[2]) : 1600;
const xs = makeAll();
fill(xs);
for (let i = 0; i < xs.length; i++) xs[i].qzmain = 2 * i + 1;
console.log(run(N, xs));
console.log(runMain(N, xs));
console.log(xs.map(rdMain).join(","));
console.log(xs.map(rd).join(","));
const c: any = xs[3];
c.qzonly = 42;
console.log(rd(c), c.k3, Object.keys(c).join(","));
(c as any).extra = 1;
console.log(rd(c), Object.keys(c).join(","));
console.log(rd_r0(xs[0]), rd_r9(xs[1]));
