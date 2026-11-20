// Representation-selection Phase 1: canonical unboxed i32 locals
// (PERRY_CANONICAL_I32_LOCALS, RFC docs/representation-selection-rfc.md).
//
// Exercises every seam of the canonical-i32 storage model against Node:
//  - an eligible strictly-i32-bounded accumulator whose value flows to boxed
//    consumers (console.log, plain-array push after the loop via `| 0`),
//  - loop-counter shapes (arr.length hoist, static bound, decrement,
//    counter declared outside the loop and read AFTER it),
//  - an unsigned `>>> 0` local with a value above 2^31 (uitofp
//    materialization),
//  - const Int32Array views (the slot.ts mix shape),
//  - a possibly-OOB int-typed-array seed (the #6898 NaN-safe ToInt32 trap),
//  - hoisted `var` redeclaration of an integer local,
//  - update expressions (postfix value) on a canonical counter,
//  - a closure-captured integer local (excluded from canonical — must stay
//    correct on the boxed protocol).

// 1. Strictly-bounded FNV-style accumulator -> boxed consumers.
function mixToArray(): string {
  const bytes = new Uint8Array(16);
  for (let i = 0; i < 16; i++) bytes[i] = (i * 37 + 11) & 0xff;
  let h = 0x811c9dc5 | 0;
  for (let i = 0; i < bytes.length; i++) {
    h = (h ^ bytes[i]) | 0;
    h = Math.imul(h, 0x01000193);
  }
  console.log(h); // bare boxed consumer: sitofp materialization must be exact
  const out: number[] = [];
  out.push(h | 0); // boxed consumer AFTER the loop through a coercing observation
  out.push((h >>> 16) & 0xffff);
  return out.join(",");
}
console.log(mixToArray());

// 2. Loop counters: hoisted arr.length bound, static bound, decrement,
//    and a counter declared OUTSIDE the loop read after it.
function counters(): string {
  const arr: number[] = [];
  for (let i = 0; i < 10; i++) arr[i] = i * 2;
  let sum = 0;
  for (let i = 0; i < arr.length; i++) sum = (sum + arr[i]) | 0;
  let down = 0;
  for (let i = 9; i >= 0; i--) down = (down + arr[i] * (i + 1)) | 0;
  let i = 0;
  for (; i < arr.length; i++) {
    sum = (sum + arr[i]) | 0; // keeps `i` index-used -> canonical-i32 counter
  }
  return sum + ":" + down + ":" + i; // `i` read after the loop
}
console.log(counters());

// 3. Unsigned `>>> 0` locals: seed above 2^31 printed directly (uitofp), and
//    a u32 mixing recurrence.
function unsignedMix(): string {
  const arr: number[] = [];
  let s = 0x9e3779b9 >>> 0; // 2654435769 > 2^31
  if (arr.length !== 0) {
    s = 0 >>> 0; // never taken; keeps `s` mutable with all-`>>>0` writes
  }
  let u = 0x9e3779b9 >>> 0;
  for (let k = 0; k < 8; k++) {
    u = (u ^ ((k * 0x85ebca6b) | 0)) >>> 0;
    u = ((u << 13) | (u >>> 19)) >>> 0;
  }
  return s + ":" + u + ":" + (u >>> 0);
}
console.log(unsignedMix());

// 4. Const Int32Array view mix (the slot.ts shape): proven in-window loads.
function taMix(): number {
  const P = new Int32Array(8);
  for (let i = 0; i < 8; i++) P[i] = Math.imul(i + 1, 0x9e3779b9 | 0);
  let l = P[0];
  for (let i = 0; i < 64; i++) {
    l = (l ^ P[i & 7]) | 0;
  }
  return l | 0;
}
console.log(taMix());

// 5. Possibly-OOB int-typed-array seed: ToInt32(undefined) = 0 must hold
//    (the NaN-safe entry conversion), with the local index-used like the
//    bcryptjs Feistel accumulators.
function feistel(lr: Int32Array, off: number, S: Int32Array): number {
  let l = lr[off];
  l ^= S[(l >>> 28) & 7];
  l = (l << 3) | (l >>> 29);
  return l | 0;
}
const lr = new Int32Array([0x12345678, 0x0fedcba9 | 0]);
const S = new Int32Array([9, 8, 7, 6, 5, 4, 3, 2]);
console.log(feistel(lr, 0, S), feistel(lr, 1, S), feistel(lr, 9, S));

// 6. Hoisted `var` redeclaration of an integer local (shared HIR id).
function redecl(flag: boolean): number {
  if (flag) {
    var v = 3 | 0;
  } else {
    var v = 7 | 0;
  }
  var q = 0;
  for (var k = 0; k < 3; k++) q = (q + v) | 0;
  return q;
}
console.log(redecl(true), redecl(false));

// 7. Update expressions on a canonical counter: postfix returns the OLD value.
function updExpr(): string {
  const arr = [10, 20, 30];
  let i = 0;
  const first = arr[i++];
  const second = arr[i++];
  const third = arr[--i]; // prefix returns the NEW value
  return first + "," + second + "," + third + "," + i;
}
console.log(updExpr());

// 8. Closure-captured integer local: excluded from canonical storage, must
//    keep exact boxed-protocol behavior.
function captured(): number {
  let t = 0;
  for (let i = 0; i < 4; i++) t = (t + i * i) | 0;
  const f = () => t + 1;
  return f();
}
console.log(captured());

// 9. Mixed: canonical accumulator alongside a plain f64 accumulator in the
//    same loop (only the strict one flips representation).
function mixedReps(): string {
  let ints = 0;
  let floats = 0.5;
  for (let i = 0; i < 6; i++) {
    ints = (ints + i * 3) | 0;
    floats = floats + i / 2;
  }
  return ints + ":" + floats;
}
console.log(mixedReps());
