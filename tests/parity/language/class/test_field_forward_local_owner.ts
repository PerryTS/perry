// A value stored into a field of a local object, read back in the same
// block: the forwarded read must be the stored value itself, and every case
// where other code can see or change the field must keep Node's result.
class P { x: number; y: number; constructor(x: number, y: number) { this.x = x; this.y = y; } }

// The forwarded shape: an inlined `put(b, p)` in a loop.
class Box { p: P | null = null; }
function put(b: Box, p: P): void { b.p = p; }
let s0 = 0;
for (let i = 0; i < 5; i++) { const b = new Box(); put(b, new P(i, i + 1)); s0 += b.p!.x + b.p!.y; }
console.log("forward", s0);

// A literal stored and read back, in a function body.
class Cell { n = 0; label = "none"; }
function literal(): string {
  let out = "";
  for (let i = 0; i < 3; i++) { const c = new Cell(); c.n = 4; c.label = "set"; out += c.n + i + c.label; }
  return out;
}
console.log("literal", literal());

// Alias write through a copy of the owner.
let sa = 0;
for (let i = 0; i < 4; i++) { const b = new Box(); const c = b; const p = new P(i, 1); const q = new P(100, 2); b.p = p; c.p = q; sa += b.p!.x; }
console.log("alias", sa);

// Second store.
let sb = 0;
for (let i = 0; i < 4; i++) { const b = new Box(); const p = new P(i, 1); const q = new P(100, 2); b.p = p; sb += b.p!.x; b.p = q; sb += b.p!.x; }
console.log("second", sb);

// Read before the store.
let sc = 0;
for (let i = 0; i < 4; i++) { const b = new Box(); const p = new P(i, 1); sc += b.p === null ? 1000 : 0; b.p = p; sc += b.p!.x; }
console.log("before", sc);

// Store in a branch.
let sd = 0;
for (let i = 0; i < 4; i++) { const b = new Box(); const p = new P(i, 1); if (i % 2 === 1) b.p = p; sd += b.p ? b.p.x : -10; }
console.log("branch", sd);

// An accessor of the same name.
class SetBox { _p: P | null = null; n = 0; set p(v: P | null) { this.n++; this._p = v ? new P(v.x * 10, 0) : null; } get p(): P | null { return this._p; } }
let se = 0;
for (let i = 0; i < 4; i++) { const b = new SetBox(); const p = new P(i, 1); b.p = p; se += b.p!.x + b.n; }
console.log("setter", se);

// A constructor that freezes the object.
class FrozenBox { p: P | null = null; constructor() { Object.freeze(this); } }
function frozen(): number {
  "use strict";
  let s = 0;
  for (let i = 0; i < 2; i++) { const b = new FrozenBox(); const p = new P(i, 1); try { b.p = p; s += b.p!.x; } catch (e) { s += 1000; } }
  return s;
}
console.log("frozen", frozen());

// A method that writes the field.
class ClearBox { p: P | null = null; clear(): void { this.p = new P(-1, -1); } }
let sg = 0;
for (let i = 0; i < 4; i++) { const b = new ClearBox(); const p = new P(i, 1); b.p = p; b.clear(); sg += b.p!.x; }
console.log("method", sg);

// The owner handed to a function that rewrites the field and keeps it.
const keep: Box[] = [];
function swap(b: Box, i: number): void { if (i % 2 === 0) b.p = new P(50, 0); keep.push(b); }
let sh = 0;
for (let i = 0; i < 4; i++) { const b = new Box(); const p = new P(i, 1); b.p = p; swap(b, i); sh += b.p!.x; }
console.log("passed", sh, keep.length, keep[0].p!.x);

// The owner lives outside the loop.
let si = 0;
const outer = new Box();
for (let i = 0; i < 4; i++) { if (i > 0) si += outer.p!.x; const p = new P(i, 1); outer.p = p; }
console.log("outer", si);

// The stored local is rebound afterwards.
let sj = 0;
for (let i = 0; i < 4; i++) { const b = new Box(); let p = new P(i, 1); b.p = p; p = new P(100, 0); sj += b.p!.x + p.x; }
console.log("rebound", sj);

// Identity and mutation through the stored object.
let sk = 0; let same = 0;
for (let i = 0; i < 4; i++) { const b = new Box(); const p = new P(i, 1); b.p = p; p.x = 7; sk += b.p!.x; if (b.p === p) same++; }
console.log("identity", sk, same);

// A constructor that keeps `this`.
let last: KeptBox | null = null;
class KeptBox { p: P | null = null; constructor() { last = this; } }
let sm = 0;
for (let i = 0; i < 4; i++) { const b = new KeptBox(); const p = new P(i, 1); b.p = p; last!.p = new P(1000, 0); sm += b.p!.x; }
console.log("kept", sm);

// The owner captured by a closure that runs later.
let sn = 0;
for (let i = 0; i < 3; i++) { const b = new Box(); const p = new P(i, 1); b.p = p; const later = () => b.p!.x; b.p = new P(9, 9); sn += later(); }
console.log("captured", sn);
