// The length floor (wpfacts `solve::arr_len`): every event that may lower
// an array's length makes it unknown; a read inside the old floor then
// keeps its tests. Node-exact with the floor on, off and in check mode.

function fresh(): number[] {
  return new Array<number>(4).fill(1);
}

// 3. pop, length =, splice, shift, each followed by an in-old-floor read.
const p = new Array<number>(4).fill(1);
if (Math.random() < 2) p.pop();
console.log("pop", p.length, p[3] ?? 7, p[2]);

const l = new Array<number>(4).fill(2);
if (Math.random() < 2) l.length = 1;
console.log("length=", l.length, l[1] ?? 7, l[3] ?? 8, l[0]);

const sp = [1, 2, 3, 4];
if (Math.random() < 2) sp.splice(0, 2);
console.log("splice", sp.length, sp[0], sp[2] ?? 9, sp[3] ?? 9);

const sh = [5, 6, 7, 8];
if (Math.random() < 2) sh.shift();
console.log("shift", sh.length, sh[0], sh[3] ?? 9);

const un = [5, 6];
if (Math.random() < 2) un.unshift(4, 3);
console.log("unshift", un.length, un[0], un[3]);

// 5. delete, an accessor element, freeze then a store.
const d = fresh();
delete d[1];
console.log("delete", d.length, d[1] ?? "hole", 1 in d);
const g = [1, 2, 3];
Object.defineProperty(g, 0, { get() { return 42; } });
console.log("accessor", g[0], g.length);
const f = [1, 2, 3];
Object.freeze(f);
try {
  "use strict";
  (f as any)[3] = 9;
  console.log("frozen store", f.length, f[3] ?? "none");
} catch (e) {
  console.log("frozen store threw", (e as Error).name);
}
console.log("frozen", f.length, f[2]);

// A loop over a length that shrinks.
const q = [1, 2, 3, 4, 5, 6, 7, 8];
let acc = 0;
for (let i = 0; i < q.length; i++) {
  acc += q[i] ?? 0;
  if (i === 2) q.length = 5;
}
console.log("shrinking loop", acc, q.length);
