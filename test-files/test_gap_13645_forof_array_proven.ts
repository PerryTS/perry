// #13645: for...of over a plain array where the program never changes array
// iteration. The proven lowering must keep exact array-iterator semantics.
const STEPS: number[] = [-17, -16, -15, -1, 1, 15, 16, 17];
function firstOver(limit: number): number {
  for (const d of STEPS) {
    if (d > limit) return d;
  }
  return -999;
}
console.log("early return", firstOver(0), firstOver(16), firstOver(100));

// Length is re-read every step: pushing during the loop extends it.
const grow: number[] = [1, 2, 3];
let seen = 0;
for (const v of grow) {
  if (grow.length < 6) grow.push(v * 10);
  seen += v;
}
console.log("grow", seen, grow.length);

// Shrinking during the loop stops early.
const shrink: number[] = [1, 2, 3, 4, 5];
const got: number[] = [];
for (const v of shrink) {
  got.push(v);
  if (v === 2) shrink.length = 3;
}
console.log("shrink", got.join(","));

// Holes read through the ordinary element get.
const holes: (number | undefined)[] = [1, , 3];
const hv: string[] = [];
for (const v of holes) hv.push(String(v));
console.log("holes", hv.join(","));

// break, continue, labelled exits and throw out of the body.
let acc = 0;
outer: for (const a of [1, 2, 3]) {
  for (const b of [10, 20, 30]) {
    if (b === 20) continue;
    if (a === 2) continue outer;
    if (a === 3) break outer;
    acc += a * b;
  }
}
console.log("labels", acc);
try {
  for (const v of STEPS) {
    if (v === 1) throw new Error("boom " + v);
  }
} catch (e) {
  console.log("throw", (e as Error).message);
}
// Writes to elements during the loop are seen.
const w: number[] = [1, 2, 3];
const wv: number[] = [];
for (const v of w) {
  wv.push(v);
  w[2] = 99;
}
console.log("write", wv.join(","));
// The loop variable is the value; per-iteration bindings in closures.
const fs: (() => number)[] = [];
for (const v of [5, 6, 7]) fs.push(() => v);
console.log("closures", fs.map((f) => f()).join(","));
// Generic arrays (strings, objects) and an empty array.
let s = "";
for (const x of ["a", "b"]) s += x;
for (const _ of [] as number[]) s += "!";
console.log("generic", s);
