// #12484 (phase 2): a literal passed to a parameter, or held in an array and
// read back by index, is born with room for the keys its consumers add. The
// room is capacity only: no key, order, enumeration or descriptor changes.

function addZ(o: any, v: number): void {
  o.z = v;
}
function addMany(o: any): void {
  o.k1 = 1; o.k2 = 2; o.k3 = 3; o.k4 = 4; o.k5 = 5;
  o.k6 = 6; o.k7 = 7; o.k8 = 8; o.k9 = 9; o.k10 = 10;
}
function describe(o: any): string {
  return JSON.stringify(o) + " " + Object.keys(o).join(",") + " " + ("z" in o) + " " +
    Object.prototype.hasOwnProperty.call(o, "z");
}

// Before and after the add: a reserved slot is never visible.
const p: any = { a: 1, b: "two" };
console.log(describe(p));
addZ(p, 3);
console.log(describe(p));
addZ(p, 4);
console.log(describe(p), p.z);

// Passed directly; several call sites with differently shaped literals.
const q: any = { x: 1 };
addZ(q, 5);
addZ({ y: 2 }, 6);
console.log(describe(q));

// More keys than the reservation cap: the rest spill, all still in order.
const m: any = { a: 0 };
addMany(m);
console.log(Object.keys(m).join(","), m.k1 + m.k10, JSON.stringify(m));
const spread = { ...m };
console.log(Object.keys(spread).length, spread.k9);

// Delete and re-add moves the key to the end, as in node.
addZ(p, 7);
delete p.a;
p.a = 9;
console.log(Object.keys(p).join(","), JSON.stringify(p));

// Array elements read back by index.
const recs: any[] = [];
for (let i = 0; i < 4; i++) recs.push({ id: i, name: "r" + i });
const lits: any[] = [{ id: 10 }, { id: 11 }];
for (let k = 0; k < 8; k++) {
  const r = recs[k & 3];
  r.seen = k;
  lits[k & 1].tag = "t" + k;
}
console.log(recs.map((r) => describe(r)).join(" | "));
console.log(JSON.stringify(lits), Object.keys(lits[0]).join(","));
const at = recs.at(-1);
at.last = true;
console.log(describe(at), Object.getOwnPropertyNames(at).join(","));

// A frozen literal from the same site rejects the add in strict mode.
const f: any = Object.freeze({ a: 1 });
try {
  (function (o: any) {
    "use strict";
    o.z = 1;
  })(f);
} catch (e) {
  console.log((e as Error).constructor.name, "z" in f);
}
