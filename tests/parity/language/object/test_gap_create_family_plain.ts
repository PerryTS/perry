// Object model step 1: `Object.assign(Object.create(P), { ... })` with P a
// module-level closed literal is a family born with its layout. The adds are
// plain (nothing in this program defines an accessor or freezes anything), so
// the object is born with its keys in source order; everything observable
// must match the ordinary operation.

const P: any = { kind: "proto", base: 10, describe() { return "d:" + this.kind; } };
const Q: any = { tag: "q" };

function make(i: number): any {
  return Object.assign(Object.create(P), { value: i, label: "n" + i, kind: "own" + (i & 3) });
}
function makeQ(i: number): any {
  return Object.assign(Object.create(Q), { a: i, b: { nested: i } });
}

const held: any[] = [];
let sum = 0;
for (let i = 0; i < 20000; i++) {
  const o = make(i);
  const q = makeQ(i);
  sum += o.value + o.base + q.a + q.b.nested;
  if (i % 1000 === 0) held.push(o, q);
  // allocation pressure between births
  const junk = [i, i + 1, { x: i }];
  sum += junk.length;
}
console.log("sum", sum, held.length);

const o = make(7);
console.log(Object.keys(o).join(","));
console.log(Object.getPrototypeOf(o) === P, Object.getPrototypeOf(makeQ(1)) === Q);
console.log(o.hasOwnProperty("value"), o.hasOwnProperty("base"), "base" in o);
console.log(o.kind, P.kind, o.describe(), o.base);
const keys: string[] = [];
for (const k in o) keys.push(k);
console.log(keys.join(","));
console.log(JSON.stringify(o), JSON.stringify(makeQ(3)));
const d = Object.getOwnPropertyDescriptor(o, "label")!;
console.log(d.value, d.writable, d.enumerable, d.configurable);

// The object keeps behaving as an ordinary object after its birth.
o.value = 99;
o.extra = "e";
delete o.label;
console.log(Object.keys(o).join(","), o.value, o.extra, o.label);

// Another creation point on the same prototype, and a plain Object.create.
const p2 = Object.create(P);
p2.value = 1;
console.log(Object.keys(p2).join(","), p2.describe(), Object.getPrototypeOf(p2) === P);

// Held objects survived the collections intact.
let ok = true;
for (let j = 0; j < held.length; j += 2) {
  const h = held[j];
  if (h.label !== "n" + h.value || Object.getPrototypeOf(h) !== P) ok = false;
}
console.log("held ok", ok);
