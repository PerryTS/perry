// EK design §3-§4: the compiled array read serves `index < P` (the dense
// prefix of the element-kind word) without bounds or hole tests. Every
// operation below can make a slot under the old prefix a hole, give it a
// descriptor, move it or cut `length`; each read must still match node.

function at(a: any[], i: number): any {
  return a[i];
}
function num(a: number[], i: number): number {
  return a[i];
}
function show(label: string, a: any[]): void {
  const parts: string[] = [];
  for (let i = 0; i < a.length + 2; i++) {
    parts.push(String(at(a, i)) + (i in a ? "" : "?"));
  }
  console.log(label, a.length, parts.join(","));
}

// Birth, then delete inside the prefix.
const d = [1, 2, 3, 4];
show("birth", d);
delete d[1];
show("delete", d);
d[1] = 20;
show("refill", d);

// Frontier stores grow the prefix; a store past the frontier leaves holes.
const f: any[] = new Array(4);
for (let i = 0; i < 4; i++) f[i] = i * 10;
show("frontier", f);
f[7] = 70;
show("past-frontier", f);
for (let i = 4; i < 7; i++) f[i] = i;
show("filled", f);

// Pushes, pops and length cuts.
const p: number[] = [];
for (let i = 0; i < 6; i++) p.push(i + 0.5);
p.pop();
p.pop();
console.log("pop", p.length, num(p, 3), num(p, 4), num(p, 5));
p.length = 2;
console.log("cut", p.length, num(p, 1), num(p, 2));
p.length = 4;
show("regrow", p);
p[3] = 3.5;
show("regrow-store", p);

// Moves: splice, copyWithin, shift, unshift, reverse, sort over holes.
const s: any[] = [0, 1, , 3, 4, 5];
s.splice(0, 1);
show("splice-del", s);
s.splice(1, 0, "x", "y");
show("splice-ins", s);
const c: any[] = [0, 1, 2, , 4, 5];
c.copyWithin(0, 2, 5);
show("copyWithin", c);
const u: any[] = [1, , 3];
u.unshift(0);
show("unshift", u);
u.shift();
u.shift();
show("shift", u);
const r: any[] = [1, 2, , 4];
r.reverse();
show("reverse", r);
const so: any[] = [3, , 1, undefined, 2];
so.sort();
show("sort", so);

// Descriptors at an index inside the prefix.
const g: any[] = [1, 2, 3, 4];
let calls = 0;
Object.defineProperty(g, 1, { get() { calls++; return "getter"; }, configurable: true });
show("getter", g);
console.log("getter calls", calls);
const fa: any[] = [1, 2, 3];
let sets = 0;
Object.defineProperty(fa, 1, {
  get() { return "acc"; },
  set(v) { sets += v; },
  configurable: true,
});
fa.fill(7);
show("fill-over-accessor", fa);
console.log("setter sum", sets);
fa.push(8);
show("push-after-accessor", fa);
const nw: any[] = [1, 2, 3];
Object.defineProperty(nw, 0, { value: "ro", writable: false });
show("define-data", nw);

// Integrity levels: reads are plain.
const fr = Object.freeze([5, 6, 7]);
console.log("frozen", at(fr as any[], 0), at(fr as any[], 2), at(fr as any[], 3));

// Numbers vs other values in a typed array of numbers.
const n: any[] = [1.5, 2.5, 3.5];
n[1] = "two";
console.log("mixed", num(n as number[], 0), num(n as number[], 1), num(n as number[], 2));

// Subclass and Array.from / map / slice results.
class Bag extends Array<number> {}
const b = new Bag();
b.push(1, 2, 3);
show("subclass", b);
show("map", [1, 2, 3].map((x) => x * 2));
show("slice", [1, , 3, 4].slice(1));
show("from", Array.from({ length: 3 }, (_, i) => i));

// Last: a prototype element makes holes observable through the chain.
(Array.prototype as any)[1] = "proto";
const h: any[] = [0, , 2];
show("proto-hole", h);
const h2 = [0, 1, 2];
delete h2[1];
show("proto-delete", h2);
delete (Array.prototype as any)[1];
