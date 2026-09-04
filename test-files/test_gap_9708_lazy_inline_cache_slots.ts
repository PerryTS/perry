// #9708: inline caches are allocated per USED site, behind an 8-byte pointer
// slot that starts null and is filled by the runtime on the site's first
// priming miss. Every inline-cache shape codegen emits has to keep resolving
// correctly across that null → allocated transition, and across the shapes
// that never prime at all (a site whose receivers are all proxies, strings,
// or small native handles keeps a null slot for the life of the program).
//
// The receivers are deliberately `any`-typed so the reads and writes go
// through the generic towers (the per-site PIC), not a class-field fast path.

// ---------------------------------------------------------------------------
// Property reads: monomorphic, polymorphic (fits the ways), megamorphic.
// ---------------------------------------------------------------------------
function readX(o: any): number {
  return o.x;
}

const mono: any = { x: 1, y: 2 };
let monoSum = 0;
for (let i = 0; i < 1000; i++) monoSum += readX(mono);
console.log("mono", monoSum);

// Five shapes with `x` at five different slots: the MRU entry plus four ways.
const polyShapes: any[] = [
  { x: 10 },
  { a: 0, x: 20 },
  { a: 0, b: 0, x: 30 },
  { a: 0, b: 0, c: 0, x: 40 },
  { a: 0, b: 0, c: 0, d: 0, x: 50 },
];
let polySum = 0;
for (let i = 0; i < 1000; i++) polySum += readX(polyShapes[i % polyShapes.length]);
console.log("poly", polySum);

// Twenty shapes: wider than the ways hold, so the site latches megamorphic
// and recovers; every read must still resolve.
const megaShapes: any[] = [];
for (let s = 0; s < 20; s++) {
  const o: any = {};
  for (let k = 0; k < s; k++) o["p" + k] = k;
  o.x = s;
  megaShapes.push(o);
}
let megaSum = 0;
for (let i = 0; i < 2000; i++) megaSum += readX(megaShapes[i % megaShapes.length]);
console.log("mega", megaSum);

// A site that can never prime: string receivers and a Proxy. Its slot stays
// null forever and every read must keep taking the miss path.
function readLength(o: any): number {
  return o.length;
}
let lenSum = 0;
for (let i = 0; i < 100; i++) lenSum += readLength("abc" + i);
const proxied: any = new Proxy({ length: 7 }, { get: (t, k) => (k === "length" ? 42 : undefined) });
for (let i = 0; i < 10; i++) lenSum += readLength(proxied);
console.log("never-primes", lenSum);

// A first read on a fresh site through a receiver that throws: the slot is
// still null when the nullish check fires, and the message must match node.
function readY(o: any): number {
  return o.y;
}
try {
  readY(null);
} catch (e: any) {
  console.log("nullish", e instanceof TypeError, e.message);
}
console.log("after-throw", readY({ y: 5 }));

// Inherited data property: the miss resolves through the prototype and the
// site primes only for own slots, so a mix keeps working.
const proto = { x: 99 };
const inheriting: any = Object.create(proto);
let inhSum = 0;
for (let i = 0; i < 100; i++) inhSum += readX(i % 2 === 0 ? inheriting : mono);
console.log("inherited", inhSum);

// ---------------------------------------------------------------------------
// Static-key writes: the four inline ways, the outlined poly tail (shapes
// 5..8) and beyond it.
// ---------------------------------------------------------------------------
function writeX(o: any, v: number): void {
  o.x = v;
}
const writeShapes: any[] = [];
for (let s = 0; s < 12; s++) {
  const o: any = {};
  for (let k = 0; k < s; k++) o["w" + k] = k;
  o.x = -1;
  writeShapes.push(o);
}
for (let round = 0; round < 50; round++) {
  for (let s = 0; s < writeShapes.length; s++) writeX(writeShapes[s], round * 100 + s);
}
console.log(
  "write-static",
  writeShapes.map((o: any) => o.x).join(","),
);

// A write site whose receivers are frozen never primes (the slot stays null)
// and must keep throwing under strict mode on every write, not just the first.
const frozen: any = Object.freeze({ x: 1 });
let frozenThrows = 0;
for (let i = 0; i < 10; i++) {
  try {
    writeX(frozen, i);
  } catch (e: any) {
    if (e instanceof TypeError) frozenThrows++;
  }
}
console.log("write-frozen", frozen.x, frozenThrows);

// ---------------------------------------------------------------------------
// Dynamic-key writes: rotating keys on one shape, then several shapes.
// ---------------------------------------------------------------------------
function writeKey(o: any, k: string, v: number): void {
  o[k] = v;
}
const dyn: any = { k0: 0, k1: 0, k2: 0, k3: 0, k4: 0 };
for (let i = 0; i < 500; i++) writeKey(dyn, "k" + (i % 5), i);
console.log("write-dyn", dyn.k0, dyn.k1, dyn.k2, dyn.k3, dyn.k4);
const dynShapes: any[] = [{ a: 1 }, { a: 1, b: 2 }, { a: 1, b: 2, c: 3 }];
for (let i = 0; i < 300; i++) writeKey(dynShapes[i % 3], "a", i);
console.log("write-dyn-shapes", dynShapes.map((o: any) => o.a).join(","));

// ---------------------------------------------------------------------------
// Symbol-keyed reads and the composed `o[sym].field` read.
// ---------------------------------------------------------------------------
const tag = Symbol("tag");
function readSym(o: any): any {
  return o[tag];
}
function readSymField(o: any): number {
  return o[tag].n;
}
const symHolder: any = { plain: 1 };
symHolder[tag] = { n: 3 };
let symSum = 0;
for (let i = 0; i < 200; i++) symSum += readSymField(symHolder);
symHolder[tag] = { n: 4 }; // mutation must invalidate the composed cache
for (let i = 0; i < 200; i++) symSum += readSymField(symHolder);
console.log("symbol", symSum, readSym(symHolder).n, readSym({}) === undefined);

// ---------------------------------------------------------------------------
// Array subclasses: `.length` and `[i]` on object-backed instances go through
// their own per-site caches.
// ---------------------------------------------------------------------------
class Stack extends Array<number> {
  peek(): number {
    return this[this.length - 1];
  }
}
function readLen(a: any): number {
  return a.length;
}
function readAt(a: any, i: number): number {
  return a[i];
}
const stack: any = new Stack();
for (let i = 0; i < 10; i++) stack.push(i * 3);
let subSum = 0;
for (let i = 0; i < 100; i++) subSum += readLen(stack) + readAt(stack, i % 10);
console.log("subclass", subSum, stack.peek(), readLen([1, 2, 3]), readAt([7, 8, 9], 1));

// ---------------------------------------------------------------------------
// Fusion: `if (base.field[i]) return base.field[i];` shares one cache
// between the fused guard and the generic read.
// ---------------------------------------------------------------------------
function firstTruthy(base: any, n: number): any {
  for (let i = 0; i < n; i++) {
    if (base.items[i]) return base.items[i];
  }
  return "none";
}
console.log(
  "fused",
  firstTruthy({ items: [0, "", 0, "hit", 5] }, 5),
  firstTruthy({ items: [0, 0] }, 2),
  firstTruthy({ items: [1] }, 1),
);

// ---------------------------------------------------------------------------
// Sites inside functions that never run cost nothing and must not disturb
// their neighbours: the module has hundreds of them.
// ---------------------------------------------------------------------------
const dead: Array<(o: any) => number> = [];
for (let i = 0; i < 4; i++) {
  dead.push((o: any) => o.a0 + o.a1 + o.a2 + o.a3 + o.a4 + o.a5 + o.a6 + o.a7 + o.a8 + o.a9);
}
console.log("dead-sites", dead.length, typeof dead[0]);
console.log("done");
