// The element read tier keyed by the receiver's array representation
// (DESIGN-p4l1 §4.2, PR-C2): reads inside and outside the floor, in value
// and in number context, of fixed, growable, holey, shifted and popped
// arrays. Every line is a value node prints too.

const K = [3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5, 8, 9, 7, 9, 3, 2, 3, 8, 4];

// In the floor, value context and number context.
function inFloor(a: number[]): number {
  let s = 0;
  for (let i = 0; i < 1000; i++) {
    const k = i & 15;
    const v = a[k];
    s = s + (v === undefined ? 1000 : v) + a[k] * 2;
  }
  return s;
}
console.log("in floor", inFloor(K));

// `??` on a read below the floor, and on one that may leave it.
function coalesce(a: number[]): number {
  let s = 0;
  for (let i = 0; i < 1000; i++) {
    const k = i & 15;
    s += (a[k] ?? 100) + (a[k + 8] ?? 100) + (a[k - 2] ?? 100);
  }
  return s;
}
console.log("coalesce", coalesce(K));

// Outside the floor: negative keys, keys past the end, a fraction.
function outside(a: number[], lo: number, hi: number): string {
  const out: string[] = [];
  for (let i = lo; i < hi; i++) {
    const v = a[i];
    out.push(v === undefined ? "u" : String(v));
  }
  return out.join(",");
}
console.log("outside", outside(K, -2, 3), outside(K, 18, 23), K[0.5], K[-1]);

// A negative key and a named key live beside the elements.
const named = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
(named as any)[-1] = 77;
(named as any).tag = "t";
console.log("named", outside(named, -2, 2), (named as any)[-1], (named as any).tag, named.length);

// Holes below the floor: `new Array(k)` without the fused fill.
const holes: number[] = new Array(20);
holes[0] = 5;
holes[19] = 6;
console.log("holes", outside(holes, 0, 3), outside(holes, 17, 21), inFloor(holes) > 0);

// Growth: an index store past the birth capacity moves the storage, and
// the binding that was read before still names the array.
const grown = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20];
function grow(a: number[], n: number): void {
  for (let i = 20; i < n; i++) a[i] = i;
}
console.log("before growth", inFloor(grown));
grow(grown, 300);
console.log("after growth", inFloor(grown), outside(grown, 298, 302), grown.length);

// Appends: no floor at all.
const pushed: number[] = [];
for (let i = 0; i < 100; i++) pushed.push(i * 2);
console.log("pushed", outside(pushed, 97, 102), outside(pushed, -1, 2), pushed.length);

// `shift` moves the front of the storage.
const shifted = [10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29];
console.log("unshifted", inFloor(shifted));
console.log("shift", shifted.shift(), shifted.shift(), shifted.length);
console.log("shifted", outside(shifted, 0, 4), outside(shifted, 16, 20));

// `pop` and a `length` store lower the length: the slots behind it read
// as absent.
const popped = [10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29];
popped.pop();
popped.pop();
console.log("popped", outside(popped, 16, 21), popped.length);
popped.length = 3;
console.log("cut", outside(popped, 0, 6), popped.length);
popped.push(1);
console.log("repushed", outside(popped, 0, 6), popped.length);

// Two arrays of different lengths through one binding: the floor is the
// shorter one's.
const short = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17];
function either(c: boolean, i: number): number {
  const a = c ? K : short;
  return a[i] ?? -1;
}
console.log("either", either(true, 19), either(false, 19), either(false, 16), either(true, 0));

// A small literal that never escapes lives in scalar slots.
function small(i: number): number {
  const d = [5, 4, 3, 2];
  return d[i & 3] + (d[i & 1] ?? 0);
}
console.log("small", small(0), small(1), small(2), small(7));

// for-of over a constant table, and over one of two tables.
const K2 = [1, 2, 3, 4];
function walk(c: boolean): number {
  const t = c ? K : K2;
  let s = 0;
  for (const d of t) s += d;
  for (const d of K) s += d * 3;
  return s;
}
console.log("walk", walk(true), walk(false));
