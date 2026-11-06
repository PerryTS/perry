// The packed-f64 fast loop clone reads its element base from the hoisted
// receiver cache rather than re-laundering the rooted slot each iteration.
// That is only sound while the clone has no safepoint, so every shape that
// leaves the clone — a side exit, a hole, a foreign index, a length change,
// a non-numeric element — must still produce node's answer.

function sumIndexed(a: number[]): number {
  let s = 0;
  for (let i = 0; i < a.length; i++) s += a[i];
  return s;
}

function sumForOf(a: number[]): number {
  let s = 0;
  for (const v of a) s += v;
  return s;
}

function scaleInPlace(a: number[], k: number): number {
  let s = 0;
  for (let i = 0; i < a.length; i++) {
    a[i] = a[i] * k;
    s += a[i];
  }
  return s;
}

function sumWithForeignIndex(a: number[], j: number): number {
  let s = 0;
  for (let i = 0; i < a.length; i++) s += a[i] + a[j];
  return s;
}

const dense: number[] = [];
for (let i = 0; i < 40; i++) dense.push(i * 1.5);
console.log("dense-indexed", sumIndexed(dense));
console.log("dense-forof", sumForOf(dense));
console.log("dense-scale", scaleInPlace(dense.slice(), 2));
console.log("dense-foreign", sumWithForeignIndex(dense, 3));

// A single element makes the loop bound 1; an empty array makes it 0.
console.log("one", sumIndexed([7.5]), sumForOf([7.5]));
console.log("empty", sumIndexed([]), sumForOf([]));

// Integer-valued members: the array is still raw-f64 but the values are
// exactly representable, which is a different canonicalization path.
const ints: number[] = [1, 2, 3, 4, 5];
console.log("ints", sumIndexed(ints), sumForOf(ints), scaleInPlace(ints.slice(), 3));

// A hole must read as undefined, so the sum is NaN through both loops.
const holed: any[] = [1, 2, 3];
holed[6] = 9;
console.log("holed-indexed", sumIndexed(holed as number[]));
console.log("holed-forof", sumForOf(holed as number[]));
console.log("holed-len", holed.length, 4 in holed);

// A non-numeric element forces the side exit out of the fast clone.
const mixed: any[] = [1, 2, "3", 4];
console.log("mixed-indexed", sumIndexed(mixed as number[]));
console.log("mixed-forof", sumForOf(mixed as number[]));

// Growing the receiver DURING the loop: the bound is hoisted at entry, so the
// appended elements must not be visited, and the base must survive the
// reallocation the growth performs.
function sumWhileGrowing(a: number[]): number {
  let s = 0;
  for (let i = 0; i < a.length; i++) {
    s += a[i];
    if (i === 0) for (let k = 0; k < 200; k++) a.push(k);
  }
  return s;
}
const grow: number[] = [1, 2, 3, 4, 5];
console.log("growing", sumWhileGrowing(grow), grow.length);

// Allocating in the body puts a real safepoint back in the loop, and the
// receiver may then move under it.
function sumAllocating(a: number[]): number {
  let s = 0;
  const keep: number[][] = [];
  for (let i = 0; i < a.length; i++) {
    keep.push([a[i], a[i] * 2]);
    s += a[i];
  }
  return s + keep.length;
}
console.log("allocating", sumAllocating(dense));

// Frozen and subclass receivers take their own paths.
const frozen = Object.freeze([1.5, 2.5, 3.5]) as number[];
console.log("frozen", sumIndexed(frozen), sumForOf(frozen));

class MyArr extends Array<number> {}
const sub: any = MyArr.from([1, 2, 3, 4]);
console.log("subclass", sumIndexed(sub), sumForOf(sub), sub.length);

// A typed array is not a plain Array and must not enter the plain clone.
const ta = new Float64Array([1.5, 2.5, 3.5]);
let taSum = 0;
for (let i = 0; i < ta.length; i++) taSum += ta[i];
console.log("typedarray", taSum);
