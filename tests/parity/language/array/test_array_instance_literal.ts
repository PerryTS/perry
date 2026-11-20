// Instance-born literals (wpfacts `bindings::BindingNodes`): a function with
// an object parameter is solved once per call site (a summary instance), and
// an Array literal its body bears is one Array per instance. What a callee
// does to that Array through a parameter (push, pop, splice, a `length`
// store, an element store of another kind) lands on the instance's Array,
// so the local's facts are those of its shared node AND of every instance
// copy: reading the shared node alone claimed the literal's birth state
// (`arr<f64,3,dense,fixed,im>`, constant length 3) and folded `a.length`
// to 3 after the callee grew the Array.
// Every G/S/K case changes the literal through a callee; every N case is a
// negative control where the birth state is right and stays claimed.

// The callees hold a loop, so the HIR does not inline them: the change
// stays behind a call.
function fill(out: number[], n: number): void {
  for (let i = 0; i < n; i++) out.push(out.length * 2);
}
function fillFront(out: number[], n: number): void {
  for (let i = 0; i < n; i++) out.unshift(100 + i);
}
function insertAt(out: number[], at: number, n: number): void {
  for (let i = 0; i < n; i++) out.splice(at, 0, 50 + i);
}
function cut(out: number[], n: number): void {
  for (let i = 0; i < n; i++) out.splice(0, 1);
}
function popN(out: number[], n: number): void {
  for (let i = 0; i < n; i++) out.pop();
}
function shiftN(out: number[], n: number): void {
  for (let i = 0; i < n; i++) out.shift();
}
function setLen(out: number[], n: number, len: number): void {
  for (let i = 0; i < n; i++) out.length = len;
}
function storeAll(out: any[], n: number, v: any): void {
  for (let i = 0; i < n; i++) out[i] = v;
}
function storeAt(out: any[], at: number, n: number, v: any): void {
  for (let i = 0; i < n; i++) out[at] = v;
}
function pushAny(out: any[], n: number, v: any): void {
  for (let i = 0; i < n; i++) out.push(v);
}
// Read-only callees.
function lenOf(a: number[]): number {
  let n = 0;
  for (let i = 0; i < a.length; i++) n++;
  return n;
}
function sum(a: number[]): number {
  let s = 0;
  for (let i = 0; i < a.length; i++) s += a[i];
  return s;
}
function show(a: any[]): string {
  let s = "";
  for (let i = 0; i < a.length; i++) s += typeof a[i] + ":" + a[i] + ",";
  return s;
}

// G1: the report's case. A push through a parameter, the length read in
// the loop and after it.
function g1(xs: number[]): number {
  let s = 0;
  const other: number[] = [5, 6, 7];
  for (const v of xs) {
    s += v + other.length;
    fill(other, 1);
  }
  return s + other.length;
}
// G2: unshift.
function g2(xs: number[]): number {
  const other: number[] = [5, 6, 7];
  let s = 0;
  for (const v of xs) {
    fillFront(other, 2);
    s += other.length * v;
  }
  return s + other.length;
}
// G3: splice that inserts.
function g3(xs: number[]): number {
  const other: number[] = [5, 6, 7];
  for (const v of xs) insertAt(other, 1, v);
  return other.length;
}
// G4: the length feeds a local, a comparison, an int32 use and a call.
function g4(xs: number[]): string {
  const other: number[] = [5, 6, 7];
  for (const v of xs) fill(other, 1);
  const n = other.length;
  const tbl = [10, 20, 30, 40, 50, 60, 70, 80];
  return (n === 3 ? "three" : "more") + (n | 0) + ":" + (n - 3) + ":" + tbl[n - 1] + ":" + lenOf(other);
}
// G5: a loop bounded by the literal's length.
function g5(xs: number[]): number {
  const other: number[] = [5, 6, 7];
  for (const v of xs) fill(other, 2);
  let c = 0;
  for (let i = 0; i < other.length; i++) c += i;
  return c;
}
// S1: pop through a parameter.
function s1(xs: number[]): number {
  const other: number[] = [5, 6, 7];
  popN(other, xs.length);
  return other.length;
}
// S2: shift; S3: a removing splice; S4: a `length` store.
function s2(xs: number[]): number {
  const other: number[] = [5, 6, 7];
  shiftN(other, xs.length);
  return other.length * 10 + lenOf(other);
}
function s3(xs: number[]): number {
  const other: number[] = [5, 6, 7, 8];
  cut(other, xs.length);
  return other.length;
}
function s4(xs: number[]): number {
  const other: number[] = [5, 6, 7];
  setLen(other, xs.length, 1);
  return other.length;
}
function s5(xs: number[]): number {
  const other: number[] = [5, 6, 7];
  setLen(other, xs.length, 9);
  return other.length;
}
// K1: an element store through the callee changes the element kind
// (Number -> String); the Array goes on to a reader.
function k1(xs: number[]): string {
  const other: any[] = [5, 6, 7];
  storeAt(other, 1, xs.length, "s");
  return other.length + "|" + show(other);
}
// K2: Number -> a non-integer Number, read back by a summing callee.
function k2(xs: number[]): number {
  const other: number[] = [5, 6, 7];
  storeAt(other, 0, xs.length, 0.5);
  storeAt(other, 2, xs.length, 2147483648.5);
  return sum(other) + other.length;
}
// K3: a store past the end (a hole) and the new length.
function k3(xs: number[]): string {
  const other: any[] = [5, 6, 7];
  storeAt(other, 6, xs.length, 9);
  return other.length + "|" + show(other);
}
// K4: a pushed value of another kind.
function k4(xs: number[]): string {
  const other: any[] = [5, 6, 7];
  pushAny(other, xs.length, "t");
  return other.length + "|" + show(other);
}
// I1: the immutable claim (`im`, constant elements): the body reads the
// literal itself after a callee overwrote it.
function i1(xs: number[]): number {
  const other: number[] = [5, 6, 7];
  storeAt(other, 0, xs.length, 40);
  let s = 0;
  for (const e of other) s += e;
  return s + other[0] + other[2] + other.length;
}
// I2: the same with only the length read in the body.
function i2(xs: number[]): number {
  const other: number[] = [5, 6, 7];
  storeAt(other, 1, xs.length, 40);
  return sum(other) * 10 + other.length;
}
// I3: the body reads constant elements of the literal a callee rewrote.
function i3(xs: number[]): number {
  const other: number[] = [5, 6, 7];
  storeAt(other, 0, xs.length, 40);
  return other[0] * 100 + other[1];
}
// I4: a whole-Array store loop in the callee (it is not summarized: its
// stores land on the shared Array).
function i4(xs: number[]): number {
  const other: number[] = [5, 6, 7];
  storeAll(other, xs.length, 40);
  let s = 0;
  for (const e of other) s += e;
  return s + other[0];
}
// T1: two call sites, two instances: one grows its literal, one does not.
function t1(xs: number[], grow: number): number {
  const other: number[] = [5, 6, 7];
  for (const v of xs) fill(other, grow);
  return other.length;
}
// R1: a recursive caller: each activation bears its own literal.
function r1(xs: number[], d: number): number {
  const other: number[] = [5, 6, 7];
  fill(other, d);
  if (d > 0) return other.length * 100 + r1(xs, d - 1);
  return other.length + xs.length;
}
// R2: the literal goes down the recursion and grows there.
function r2down(acc: number[], d: number): void {
  for (let i = 0; i < d; i++) acc.push(d);
  if (d > 0) r2down(acc, d - 1);
}
function r2(xs: number[]): number {
  const other: number[] = [5, 6, 7];
  r2down(other, xs.length);
  return other.length;
}
// C1: the literal is born in a callee the facts expand into its caller's
// summary: the caller's instances bear it.
function c1made(xs: number[]): number {
  const made: number[] = [5, 6, 7];
  fill(made, xs.length);
  return made.length;
}
function c1(xs: number[], ys: number[]): number {
  let s = 0;
  for (const y of ys) s += y;
  return c1made(xs) * 1000 + c1made(ys) + s;
}
// C2: a guarded counter beside the literal: the instances' intervals of
// `d` and `k` are joined with the shared node's.
function c2(xs: number[], k: number): number {
  const other: number[] = [5, 6, 7];
  for (const v of xs) fill(other, 1);
  const tbl = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120];
  let s = 0;
  let t = k | 0;
  while (t >= 0 && t < 10) {
    const d = t;
    s += tbl[d] + other.length;
    t += 3;
  }
  return s;
}
// O1: an object literal whose field a callee rewrites.
function bump(o: { n: any; tag: string }, k: number, v: any): void {
  for (let i = 0; i < k; i++) o.n = v;
}
function o1(xs: number[]): string {
  const o = { n: 1, tag: "a" };
  bump(o, xs.length, "str");
  return typeof o.n + ":" + o.n + ":" + o.tag;
}
// N1: negative control: only read-only callees; the constant length is
// right (and stays folded).
function n1(xs: number[]): number {
  const other: number[] = [5, 6, 7];
  let s = 0;
  for (const v of xs) s += v + other.length + lenOf(other);
  return s + other.length;
}
// N2: no callee at all.
function n2(xs: number[]): number {
  const other: number[] = [5, 6, 7];
  let s = xs.length;
  for (let i = 0; i < other.length; i++) s += other.length;
  return s;
}
// N3: the callee changes ANOTHER literal of the same body.
function n3(xs: number[]): number {
  const kept: number[] = [1, 2, 3, 4];
  const grown: number[] = [5, 6, 7];
  for (const v of xs) fill(grown, 1);
  return kept.length * 1000 + sum(kept) * 10 + grown.length;
}
// N4: a module-level literal no function changes.
const TABLE: number[] = [3, 1, 4, 1, 5];
function n4(xs: number[]): number {
  let s = 0;
  for (const v of xs) s += TABLE.length + v;
  return s + sum(TABLE);
}

console.log("g1", g1([1, 4, 7, 10, 13]), g1([]));
console.log("g2", g2([1, 2, 3]), g2([]));
console.log("g3", g3([1, 2]), g3([]));
console.log("g4", g4([1, 2, 3]), g4([]));
console.log("g5", g5([1, 2]), g5([]));
console.log("s1", s1([1]), s1([1, 2, 3, 4]), s1([]));
console.log("s2", s2([1, 2]), s2([]));
console.log("s3", s3([1, 2, 3]), s3([]));
console.log("s4", s4([1]), s4([]));
console.log("s5", s5([1]), s5([]));
console.log("k1", k1([1, 2]), k1([]));
console.log("k2", k2([1, 2, 3]), k2([]));
console.log("k3", k3([1]), k3([]));
console.log("k4", k4([1, 2]), k4([]));
console.log("i1", i1([1, 2, 3]), i1([]));
console.log("i2", i2([1, 2, 3]), i2([]));
console.log("i3", i3([1]), i3([]));
console.log("i4", i4([1, 2]), i4([]));
console.log("t1", t1([1, 2], 2), t1([1, 2], 0));
console.log("r1", r1([1], 3), r1([], 0));
console.log("r2", r2([1, 2, 3]), r2([]));
console.log("c1", c1([1, 2], [3]), c1([], []));
console.log("c2", c2([1, 2], 1), c2([], 2), c2([1], -1), c2([1, 2, 3], 4));
console.log("o1", o1([1]), o1([]));
console.log("n1", n1([1, 2]), n1([]));
console.log("n2", n2([1, 2]), n2([]));
console.log("n3", n3([1, 2]), n3([]));
console.log("n4", n4([1, 2]), n4([]));
