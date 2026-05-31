// Gap test: Array / TypedArray callback semantics (#2797 / #2798 / #2799).
// Covers: callbacks receiving (element, index, array) and reducers receiving
// (accumulator, currentValue, currentIndex, array); reduce with/without an
// initial value; reduce/reduceRight on an empty array (and empty typed array)
// with no initial value throwing TypeError.
//
// Notes on scope:
//  - Plain-array callbacks assert the spec third/fourth argument is the
//    receiver array via `a === arr` (Perry boxes array locals consistently).
//  - TypedArray cases use Int32Array (Perry aliases Uint8Array to its Buffer
//    representation, tracked by #2447) and live inside a function so the
//    typed array is not stored in a module-level F64 slot. They observe the
//    receiver argument via `a.length` / `a[i]` because Perry's typed-array
//    local representation is not pointer-identical to the boxed receiver yet
//    (a separate representation gap, not a callback-arity gap).

const arr = [10, 20, 30];

// --- find family: (value, index, array) + traversal order ---
const findCalls: string[] = [];
const found = arr.find((v, i, a) => {
  findCalls.push(`${v}|${i}|${a === arr}`);
  return v === 20;
});
console.log("find:", found, findCalls.join(","));

const fli = arr.findLastIndex((v, i, a) => a === arr && v >= 20);
console.log("findLastIndex:", fli);

const flastCalls: string[] = [];
const flast = arr.findLast((v, i, a) => {
  flastCalls.push(`${v}|${i}|${a === arr}`);
  return v < 30;
});
console.log("findLast:", flast, flastCalls.join(","));

// --- iterative: map / filter / some / every / forEach receive (v, i, array) ---
const mapped = arr.map((v, i, a) => v + (a === arr ? i : 100));
console.log("map:", JSON.stringify(mapped));

const filtered = arr.filter((v, i, a) => a === arr && i >= 1);
console.log("filter:", JSON.stringify(filtered));

console.log("some:", arr.some((v, i, a) => a === arr && v === 20));
console.log("every:", arr.every((v, i, a) => a === arr && v >= 10));

const feCalls: string[] = [];
arr.forEach((v, i, a) => {
  feCalls.push(`${v}|${i}|${a === arr}`);
});
console.log("forEach:", feCalls.join(","));

const flatMapped = arr.flatMap((v, i, a) => [v, a === arr ? i : -1]);
console.log("flatMap:", JSON.stringify(flatMapped));

// --- reduce / reduceRight: (acc, value, index, array) + init / no-init ---
const rCalls: string[] = [];
const reduced = arr.reduce((acc, v, i, a) => {
  rCalls.push(`${acc}|${v}|${i}|${a === arr}`);
  return acc + v;
});
console.log("reduce:", reduced, rCalls.join(","));

const reducedInit = arr.reduce((acc, v, i, a) => acc + v + (a === arr ? 0 : 1000), 5);
console.log("reduce init:", reducedInit);

const rrCalls: string[] = [];
const reducedRight = arr.reduceRight((acc, v, i, a) => {
  rrCalls.push(`${acc}|${v}|${i}|${a === arr}`);
  return acc + v;
});
console.log("reduceRight:", reducedRight, rrCalls.join(","));

// --- empty array with no initial value throws TypeError ---
try {
  ([] as number[]).reduce((a, b) => a + b);
  console.log("empty reduce: no throw");
} catch (e) {
  console.log("empty reduce:", (e as Error).name, (e as Error).message);
}

try {
  ([] as number[]).reduceRight((a, b) => a + b);
  console.log("empty reduceRight: no throw");
} catch (e) {
  console.log("empty reduceRight:", (e as Error).name, (e as Error).message);
}

// --- TypedArray callback semantics (Int32Array, function-scoped) ---
function typedArrayCases(): void {
  const ta = new Int32Array([10, 20, 30]);

  // find observes (value, index, array); array via a.length / a[i].
  console.log("ta.find:", ta.find((v, i, a) => v === 20 && a.length === 3 && a[i] === v));
  console.log("ta.findIndex:", ta.findIndex((v, i, a) => a.length === 3 && v === 30));
  console.log("ta.findLast:", ta.findLast((v, i, a) => a.length === 3 && v < 30));

  // map / filter return same-kind typed arrays.
  console.log("ta.map:", Array.from(ta.map((v, i, a) => v + i + (a.length === 3 ? 0 : 100))).join(","));
  console.log("ta.filter:", Array.from(ta.filter((v, i, a) => a.length === 3 && i > 0)).join(","));

  console.log("ta.some:", ta.some((v, i, a) => a.length === 3 && v === 20));
  console.log("ta.every:", ta.every((v, i, a) => a.length === 3 && v >= 10));

  const feCalls: string[] = [];
  ta.forEach((v, i, a) => {
    feCalls.push(`${v}|${i}|${a.length}`);
  });
  console.log("ta.forEach:", feCalls.join(","));

  // reduce / reduceRight with the (acc, value, index, array) reducer.
  console.log("ta.reduce:", ta.reduce((acc, v, i, a) => acc + v + (a.length === 3 ? 0 : 1000)));
  console.log("ta.reduce init:", ta.reduce((acc, v, i, a) => acc + v + (a.length === 3 ? 0 : 1000), 5));
  console.log("ta.reduceRight:", ta.reduceRight((acc, v, i, a) => acc + v + (a.length === 3 ? 0 : 1000)));

  try {
    new Int32Array(0).reduce((a, b) => a + b);
    console.log("empty ta.reduce: no throw");
  } catch (e) {
    console.log("empty ta.reduce:", (e as Error).name, (e as Error).message);
  }
}
typedArrayCases();
