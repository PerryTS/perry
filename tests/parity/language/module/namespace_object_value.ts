// `import * as m` used as a value is the module namespace object: the exports
// sorted on a null prototype, tagged `Module`, with live `let`
// bindings and class members, the same object in sync and async bodies (#15385).
import * as m from "./fixtures/namespace_object_value/lib.ts";

const ns: any = m;
function take(x: any) {
  return Object.keys(x).length + ":" + x.a + ":" + x.b() + ":" + x.obj.z;
}
console.log(typeof m, Object.keys(m).join(","), ns === m, take(m));
console.log(Object.prototype.toString.call(m), Object.getPrototypeOf(m) === null);
console.log(new m.K().v, typeof m.K, new ns.K().v, ns.default, JSON.stringify(Object.entries(m).filter(([k]) => k === "a" || k === "obj")));
m.bump();
console.log(ns.counter, m.counter);
async function run() {
  const n2: any = m;
  console.log("async", typeof m, Object.keys(m).join(","), n2 === ns, m.a, m.b(), n2.counter, new m.K().v);
}
run();
