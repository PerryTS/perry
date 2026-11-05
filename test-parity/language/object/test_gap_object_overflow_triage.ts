// Object model step 4 triage: behaviours of objects with an overflow that
// layout-only fast paths got wrong (deleted keys, undefined spilled keys,
// repeated JSON.parse, structured clone, Array-subclass deletes, inherited
// read-only keys, own @@iterator after a delete, megamorphic reads).

// Deleting a typed (number) field reads undefined, and a re-add returns.
const o: any = { a: 1.5, b: "s", c: 2.5 };
delete o.a;
console.log(o.a, o.a === undefined, "a" in o, Object.keys(o).join(","));
o.a = 7;
console.log(o.a, Object.keys(o).join(","));

// The same JSON text parsed twice yields two complete, distinct records.
const text =
  '{"name":"long-enough-to-cross-the-small-template-threshold","tags":["alpha","beta"]}';
const p1 = JSON.parse(text);
const p2 = JSON.parse(text);
console.log(JSON.stringify(p1), JSON.stringify(p2), p1 !== p2, p1.tags !== p2.tags);

// Structured clone of a parsed record and of an object with a deleted key.
console.log(JSON.stringify(structuredClone(p1)));
const d: any = { k0: 0, k1: 1, k2: 2, k3: 3 };
delete d.k1;
console.log(JSON.stringify(structuredClone(d)), Object.keys(structuredClone(d)).length);

// An Array subclass with a deleted element. (`1 in m` after the delete is
// true on main and here; that pre-existing gap is not this file's subject.)
class MyArr extends Array<number> {}
const m = new MyArr();
m.push(1, 2, 3);
delete (m as any)[1];
console.log(m[1], m.length, m[2]);

// A spilled key holding undefined survives the object getting an overflow.
const w: any = {};
for (let i = 0; i < 20; i++) w["k" + i] = i === 15 ? undefined : i;
delete w.k0;
console.log("k15" in w, Object.keys(w).length, w.k15, w.k19);

// An inherited read-only key blocks an assignment on the child.
const proto: any = { x: 1, y: 2 };
delete proto.y;
Object.defineProperty(proto, "x", { writable: false });
const child = Object.create(proto);
let threw = false;
try {
  child.x = 5;
} catch (err) {
  threw = err instanceof TypeError;
}
console.log(threw, child.x, Object.keys(child).length, Object.prototype.hasOwnProperty.call(child, "x"));

// An array's own @@iterator: deleted, then defined again.
const arr: any = [1, 2];
arr[Symbol.iterator] = function* () {
  yield 9;
};
console.log([...arr].join(","));
delete arr[Symbol.iterator];
console.log([...arr].join(","));
arr[Symbol.iterator] = function* () {
  yield 7;
};
console.log([...arr].join(","));

// One read site over many layouts, a third of them with `end` deleted.
const objs: any[] = [];
for (let i = 0; i < 40; i++) {
  const q: any = { pos: 1, end: 2, kind: i };
  q["x" + i] = 1;
  objs.push(q);
}
for (let i = 0; i < 40; i += 3) delete objs[i].end;
let sum = 0;
let missing = 0;
for (let r = 0; r < 4; r++) {
  for (const q of objs) {
    const v = q.end;
    if (v === undefined) missing++;
    else sum += v;
  }
}
console.log(sum, missing);

// Computed keys added after birth, then a fixed key deleted and re-added.
const e: any = { first: 1, second: 2 };
for (let i = 0; i < 5; i++) e["dyn" + i] = i;
delete e.first;
e.first = 10;
console.log(Object.keys(e).join(","), e.first, e.dyn4);
console.log(p1.name.length, p1.tags.join("+"));
