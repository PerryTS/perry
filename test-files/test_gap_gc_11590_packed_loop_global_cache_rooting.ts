// #11590: a packed-f64 range loop at module scope caches every loop-invariant
// module global it reads in an entry alloca, for both loop clones. When the
// global holds a heap receiver — here an array grown from `[]` by `d[j] = v`,
// so the entry guard fails and the SLOW clone runs, which polls for GC on its
// back-edge and grows the array through `js_dyn_index_set_strict` — that copy
// must be a GC root. It was a bare alloca: an evacuating minor rewrote the
// global but not the cache, and the next store dereferenced from-space
// (SIGSEGV under PERRY_GC_SCHEDULE_SEED + PERRY_GC_PROTECT_FROMSPACE=1).
//
// Each global is read BY NAME inside a function (`snapshot`), which is what
// keeps it a module global instead of a main()-local. Every value printed is a checksum Node agrees on;
// without the GC knobs this is a plain behavioural test.

let grown: any = [];
for (let j = 0; j < 80; j++) grown[j] = (j * 7) & 0xfffff;

let grownTyped: number[] = [];
for (let j = 0; j < 96; j++) grownTyped[j] = j * 3 + 1;

let pushed: any = [];
for (let j = 0; j < 64; j++) pushed.push(j);
for (let j = 0; j < 64; j++) pushed[j] = pushed[j] * 2 + 1;

// Two grown globals in one loop body, plus a numeric global (the cache's
// original purpose: a non-pointer that stays a bare, promotable slot).
const scale = 3;
let left: any = [];
let right: any = [];
for (let j = 0; j < 72; j++) left[j] = j;
for (let j = 0; j < 72; j++) right[j] = left[j] * scale + 1;

// Grow past several capacity doublings so the array is forwarded repeatedly.
let big: any = [];
for (let j = 0; j < 2000; j++) big[j] = j % 97;

function sumAny(d: any, n: number): number {
  let s = 0;
  for (let j = 0; j < n; j++) s += d[j];
  return s;
}
function sumTyped(d: number[], n: number): number {
  let s = 0;
  for (let j = 0; j < n; j++) s += d[j];
  return s;
}
function report(name: string, d: any, n: number): void {
  console.log(name, d.length, sumAny(d, n), d[0], d[n - 1]);
}

function snapshot(): string {
  return [grown.length, grownTyped.length, pushed.length, left.length, right.length, big.length].join(",");
}
console.log("lengths", snapshot());

report("grown", grown, 80);
console.log("grownTyped", grownTyped.length, sumTyped(grownTyped, 96), grownTyped[95]);
report("pushed", pushed, 64);
report("left", left, 72);
report("right", right, 72);
report("big", big, 2000);

// Churn after the fact: every array must still be intact and readable.
let acc = 0;
for (let r = 0; r < 200; r++) {
  const tmp: number[] = [];
  for (let j = 0; j < 50; j++) tmp.push(r + j);
  acc = (acc + sumAny(grown, 80) + sumTyped(grownTyped, 96) + tmp[49]) % 1000000007;
}
console.log("churn", acc, sumAny(big, 2000), sumAny(right, 72));
