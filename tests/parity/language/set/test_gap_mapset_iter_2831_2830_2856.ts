// #2831: insertion order preserved after delete (+ re-add appends at end)
console.log("set del first:", JSON.stringify(Array.from((() => {
  const s = new Set([1, 2, 3]);
  s.delete(1);
  return s;
})())));
console.log("set del mid:", JSON.stringify(Array.from((() => {
  const s = new Set([1, 2, 3, 4]);
  s.delete(2);
  return s;
})())));
console.log("map del mid:", JSON.stringify(Array.from((() => {
  const m = new Map([[1, "a"], [2, "b"], [3, "c"], [4, "d"]]);
  m.delete(2);
  return m.keys();
})())));
console.log("map del readd:", JSON.stringify(Array.from((() => {
  const m = new Map([[1, "a"], [2, "b"]]);
  m.delete(1);
  m.set(1, "z");
  return m.entries();
})())));

// #2830: forEach arg arity (value, key, collection) + thisArg + undefined return
const m = new Map([["k", "v"]]);
const ctx = { tag: "ctx" };
const out: unknown[] = [];
const ret = m.forEach(function (this: any, value, key, self) {
  out.push(this.tag, value, key, self === m);
}, ctx);
console.log("map forEach:", JSON.stringify(out), ret === undefined);

const s = new Set(["x"]);
const out2: unknown[] = [];
s.forEach(function (this: any, value, key, self) {
  out2.push(this.tag, value, key, self === s);
}, ctx);
console.log("set forEach:", JSON.stringify(out2));

// #2856: real iterator objects
import { types } from "node:util";
const mk = new Map([[1, "a"], [2, "b"]]).keys();
console.log("map keys array:", Array.isArray(mk));
console.log("map keys next type:", typeof (mk as any).next);
console.log("map keys util:", types.isMapIterator(mk));
console.log("map keys next:", JSON.stringify((mk as any).next()), JSON.stringify((mk as any).next()));

const se = new Set(["x", "y"]).entries();
console.log("set entries array:", Array.isArray(se));
console.log("set entries next type:", typeof (se as any).next);
console.log("set entries util:", types.isSetIterator(se));
console.log("set entries next:", JSON.stringify((se as any).next()), JSON.stringify((se as any).next()));

// default iterators
console.log("map default:", new Map([[1, "a"]])[Symbol.iterator]().next().value.join(":"));
console.log("set default:", new Set(["x"])[Symbol.iterator]().next().value);

// spread + for-of of iterator results
console.log("spread map entries:", JSON.stringify([...new Map([[1, "a"], [2, "b"]]).entries()]));
const keysOut: number[] = [];
for (const k of new Set([10, 20, 30]).keys()) keysOut.push(k);
console.log("for-of set keys:", JSON.stringify(keysOut));
