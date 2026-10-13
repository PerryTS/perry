// Element-is-the-proof: a numeric-context array read (`number[]` annotation)
// must give the JS answer whatever the element really is and whichever layout
// the array was born in (raw-f64 literal vs `[]` + push, boxed).

// `i | 0` is an integer-array-index proof, so these reads take the guarded
// numeric tier (the one that used to require the raw-f64 layout bit).
function rd(a: number[], i: number): number {
  return a[i | 0] + 1;
}
function rdMul(a: number[], i: number): number {
  return a[i | 0] * 2;
}
function rdOr(a: number[], i: number): number {
  return (a[i | 0] + 0.5) | 0;
}
function rdNeg(a: number[], i: number): number {
  return -a[i | 0];
}
function rdAcc(a: number[], i: number): number {
  let s = 0;
  s += a[i | 0];
  return s;
}
function rdCmp(a: number[], i: number): boolean {
  return a[i | 0] > 5;
}
function rdFloor(a: number[], i: number): number {
  return Math.floor(a[i | 0]);
}
function rdEq(a: number[], i: number): boolean {
  return a[i | 0] === 7;
}
// the same read with an index that carries no integer proof
function rdDyn(a: number[], i: number): number {
  return a[i] + 1;
}
function show(tag: string, a: number[], n: number): void {
  const out: string[] = [];
  for (let i = 0; i < n; i++) {
    out.push(String(rd(a, i)) + "," + String(rdMul(a, i)) + "," + String(rdOr(a, i)) + "," + String(rdNeg(a, i)) + "," + String(rdDyn(a, i)) + "," + String(rdAcc(a, i)) + "," + String(rdCmp(a, i)) + "," + String(rdFloor(a, i)) + "," + String(rdEq(a, i)));
  }
  console.log(tag, out.join(" | "));
}

// raw-born literal and boxed-born push arrays, same numeric contents
const rawLit: number[] = [3, 10.5, -7, 0, NaN, 1e300];
const pushed: number[] = [];
for (const v of [3, 10.5, -7, 0, NaN, 1e300]) pushed.push(v);
show("raw ", rawLit, 8);
show("boxed", pushed, 8);

// holes and out-of-bounds, both layouts
const holeRaw: number[] = [1, 2, 3];
holeRaw.length = 6;
holeRaw[4] = 9;
show("hole raw", holeRaw, 7);
const holeBoxed: number[] = [];
holeBoxed.push(1);
holeBoxed.push(2);
holeBoxed[4] = 9;
show("hole boxed", holeBoxed, 6);
// a hole that resolves through the prototype
(Array.prototype as any)[3] = 100;
show("proto hole raw", holeRaw, 5);
show("proto hole boxed", holeBoxed, 5);
delete (Array.prototype as any)[3];

// a non-number element written through an annotation hole (any)
function poison(a: number[], i: number, v: any): void {
  (a as any)[i] = v;
}
for (const layout of ["raw", "boxed"]) {
  for (const v of ["7", "x", undefined, null, true, { valueOf: () => 41 }, [5], 5n as any, 2147483647 | 0]) {
    const a: number[] = layout === "raw" ? [1, 2, 3, 4] : [];
    if (layout === "boxed") { a.push(1); a.push(2); a.push(3); a.push(4); }
    poison(a, 2, v);
    let r: string;
    try { r = [rd(a, 2), rdMul(a, 2), rdOr(a, 2), rdNeg(a, 2), rdAcc(a, 2), rdCmp(a, 2), rdFloor(a, 2), rdEq(a, 2), rd(a, 1)].map(String).join(","); }
    catch (e: any) { r = "throw " + e.constructor.name; }
    console.log("poison", layout, typeof v, r);
  }
}

// getter on an index of an exotic receiver (accessor property on an array)
for (const layout of ["raw", "boxed"]) {
  const a: number[] = layout === "raw" ? [1, 2, 3] : [];
  if (layout === "boxed") { a.push(1); a.push(2); a.push(3); }
  let calls = 0;
  Object.defineProperty(a, 1, { get() { calls++; return 20 + calls; }, configurable: true });
  console.log("getter", layout, rd(a, 1), rd(a, 1), rdMul(a, 1), calls, rd(a, 0), rd(a, 2));
}
// array-like object and proxy typed as number[]
const like = { length: 3, 0: 5, 1: "6", get 2() { return 7; } } as any as number[];
show("arraylike", like, 4);
const prox = new Proxy([1, 2, 3], { get(t, k, r) { return typeof k === "string" && /^\d+$/.test(k) ? Number(k) * 10 + 0.25 : Reflect.get(t, k, r); } }) as number[];
show("proxy", prox, 4);
// Int32 boxes (bitwise results) stored then read back
const ints: number[] = [];
for (let i = 0; i < 5; i++) ints.push((i * 1000003) | 0);
ints.push(-1 >>> 0);
show("int32", ints, 6);
// stale layout: raw array demoted by a later non-number store, then re-read
const dem: number[] = [1, 2, 3];
console.log("before", rd(dem, 1), rdMul(dem, 1));
(dem as any)[1] = "9";
console.log("after", rd(dem, 1), rdMul(dem, 1), rd(dem, 0));
(dem as any)[1] = { valueOf: () => 4 };
console.log("obj", rd(dem, 1), rdMul(dem, 1), rd(dem, 2));
dem.push(8);
console.log("push", rd(dem, 3), JSON.stringify(dem));
