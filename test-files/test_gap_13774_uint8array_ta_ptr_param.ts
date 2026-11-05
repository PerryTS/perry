// #13774: a `Uint8Array` argument reaches its callee through the typed-array
// spec ABI (`TaPtr`), like the other numeric kinds. Reads past the end are
// `undefined` (so a sum becomes NaN), and stores wrap with ToUint8.
//
// Each array is passed only to the functions written for it, and its other
// uses are element reads with literal keys and `.length`, so `sumStep`,
// `sumRows`, `fill` and `bump` are specialized with their array as a `TaPtr`
// parameter (`readAt` keeps the boxed entry). A key computed from `.length` or
// a method call such as `join` would keep the boxed ABI for all of them.

function sumStep(a: Uint8Array, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i += 1) s += a[i];
  return s;
}

function sumRows(a: Uint8Array, w: number, rows: number): number {
  let s = 0;
  for (let r = 0; r < rows; r++) {
    for (let k = r; k < r + w; k++) s += a[k];
  }
  return s;
}

function fill(a: Uint8Array, n: number, v: number): void {
  for (let i = 0; i < n; i += 1) a[i] = v + i;
}

function bump(a: Uint8Array, n: number): void {
  for (let i = 0; i < n; i += 1) a[i] += 200;
}

function readAt(a: Uint8Array, i: number): number | undefined {
  return a[i];
}

const n = 1 << 10;
const bytes = new Uint8Array(n);
for (let i = 0; i < bytes.length; i++) bytes[i] = (i * 31) & 255;

console.log("step", sumStep(bytes, bytes.length));
console.log("step past end", sumStep(bytes, bytes.length + 1));
console.log("step empty", sumStep(bytes, 0));
console.log("rows", sumRows(bytes, 16, 64));
console.log("rows past end", sumRows(bytes, 16, n));

const scratch = new Uint8Array(n >> 4);
fill(scratch, scratch.length, 250);
console.log("fill wraps", scratch[0], scratch[5], scratch[6], scratch[63]);
fill(scratch, 4, -2);
console.log("fill negative", scratch[0], scratch[1], scratch[2], scratch[3]);
fill(scratch, 2, 3.7);
console.log("fill fraction", scratch[0], scratch[1]);
fill(scratch, 1, NaN);
console.log("fill NaN", scratch[0]);
fill(scratch, scratch.length + 3, 1);
console.log("fill past end", scratch.length, scratch[63], scratch[64]);

const counters = new Uint8Array(8);
bump(counters, counters.length);
bump(counters, counters.length);
console.log("bump", counters[0], counters[3], counters[7]);

const probe = new Uint8Array(n >> 8);
probe[1] = 7;
console.log("read", readAt(probe, 1), readAt(probe, probe.length), readAt(probe, -1));

// The store half: a proven-integer value takes the inline byte store, and
// every store keeps ToUint8's wrap plus the silent no-op outside the range.
function paint(a: Uint8Array, n: number): void {
  for (let i = 0; i < n; i += 1) a[i] = i & 255;
}

function shiftDown(a: Uint8Array, n: number): void {
  for (let i = 0; i < n; i += 1) a[i - 1] = i;
}

const stamps = new Uint8Array(300);
paint(stamps, stamps.length);
console.log(
  "paint",
  stamps[0],
  stamps[1],
  stamps[254],
  stamps[255],
  stamps[256],
  stamps[299],
);
paint(stamps, 3);
console.log("paint prefix", stamps[0], stamps[2], stamps[3]);
paint(stamps, stamps.length + 2);
console.log("paint past end", stamps.length, stamps[299]);

const edged = new Uint8Array(8);
edged[0] = 111;
shiftDown(edged, 9);
console.log("shift down", edged[0], edged[1], edged[7]);
shiftDown(edged, 0);
console.log("shift down empty", edged[0]);

let total = 0;
for (let r = 0; r < 50; r++) total += sumStep(bytes, bytes.length) + sumRows(bytes, 8, 32);
console.log("repeated", total);
console.log("typeof past end", typeof sumStep(bytes, bytes.length + 1));
