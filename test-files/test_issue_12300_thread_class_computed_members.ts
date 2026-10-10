// #12300: a perry/thread worker never runs module init, so no class
// definition is evaluated in its realm; it inherits the evaluations its
// spawner sees. A class's computed members must therefore carry the keys the
// spawner's evaluation gave them — every member, in ClassBody order, as if the
// worker's own module init had evaluated the class. A worker that saw a
// computed member unnamed stopped its prototype there and lost every later
// member ("computed is not a function", an undefined [Symbol.iterator]).
//
// perry-only (`perry/thread` has no Node equivalent): expected output in
// test-parity/expected/. Every worker line must equal the main thread's.
import { parallelMap, spawn } from "perry/thread";
import { C, exercise } from "./_helpers/class_computed_members_12300.ts";

// Same module: a literal method BEFORE a computed generator, and one after.
const k2 = "tw" + "o";
class D {
  v = 4;
  first() { return "f" + this.v; }
  *[Symbol.iterator]() { yield this.v; }
  [k2]() { return "t" + this.v; }
  after() { return "a" + this.v; }
}
function local(o: any): string {
  return JSON.stringify([o.first(), [...o], o.two(), o.after(), Object.getOwnPropertyNames(Object.getPrototypeOf(o)).join(",")]);
}

const mainC = exercise(new C());
const mainD = local(new D());
console.log("main C " + mainC);
console.log("main D " + mainD);

// The imported class is first constructed INSIDE the workers.
const types = parallelMap([1, 2], (n: number) => {
  const o: any = new C();
  return [typeof o.computed, typeof o.later, typeof o[Symbol.iterator], typeof o.computedG, typeof o.last, n].join(" ");
});
console.log("types " + types.join(" | "));

const mapped = parallelMap([1, 2, 3, 4], (n: number) => exercise(new C()) + " " + local(new D()));
let same = true;
for (const m of mapped) if (m !== mainC + " " + mainD) same = false;
console.log("parallelMap " + mapped.length + " same " + same);
if (!same) console.log(mapped[0]);

const spawned = await spawn(() => exercise(new C()) + " " + local(new D()));
console.log("spawn same " + (spawned === mainC + " " + mainD));
if (spawned !== mainC + " " + mainD) console.log(spawned);

console.log("main again " + (exercise(new C()) === mainC && local(new D()) === mainD));
