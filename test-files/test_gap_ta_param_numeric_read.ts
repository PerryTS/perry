// Reading a typed-array element through a *parameter* in NUMERIC (arithmetic)
// context — `n += S[i]`, not `(a ^ S[i]) | 0`. perry-codegen
// expr/ta_param_f64_read.rs lowers such a read to an inline checked f64 load
// (guard: pointer + inline-storage PERRY_TA_VIEW_GUARD + kind-cache; header
// bounds check; bare load widened to f64; OOB/negative -> the TAG_UNDEFINED
// double == js_typed_array_get; slow fallback js_typed_array_read_f64 for
// view/detached/wrong-kind). Bit-exact drop-in for the runtime getter, so every
// line must match `node --experimental-strip-types` exactly. This is the
// bcryptjs `_encipher` shape (`n = S[l>>>24]; n += S[...]`).

// ---- additive reads, one per numeric kind, in a loop ----
function sumI8(S: Int8Array, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s += S[i & 7];
  return s;
}
function sumU8(S: Uint8Array, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s += S[i & 7];
  return s;
}
function sumU8C(S: Uint8ClampedArray, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s += S[i & 7];
  return s;
}
function sumI16(S: Int16Array, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s += S[i & 7];
  return s;
}
function sumU16(S: Uint16Array, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s += S[i & 7];
  return s;
}
function sumI32(S: Int32Array, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s += S[i & 7];
  return s;
}
function sumU32(S: Uint32Array, n: number): number {
  // U32 must widen UNSIGNED: 0xffffffff -> 4294967295, not -1.
  let s = 0;
  for (let i = 0; i < n; i++) s += S[i & 7];
  return s;
}
function sumF32(S: Float32Array, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s += S[i & 3];
  return s;
}
function sumF64(S: Float64Array, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s += S[i & 3];
  return s;
}

const i8 = Int8Array.from([-5, 100, -128, 7, 127, -1, 42, 99]);
const u8 = Uint8Array.from([1, 200, 255, 7, 128, 0, 42, 99]);
const u8c = Uint8ClampedArray.from([1, 200, 255, 7, 128, 0, 42, 99]);
const i16 = Int16Array.from([-5, 30000, -32768, 7, 32767, -1, 42, 999]);
const u16 = Uint16Array.from([1, 60000, 65535, 7, 32768, 0, 42, 999]);
const i32 = Int32Array.from([-5, 100000, -2000000000, 7, 0x7fffffff, -1, 42, 999]);
const u32 = Uint32Array.from([1, 4000000000, 0xffffffff, 7, 0x80000000, 0, 42, 999]);
const f32 = Float32Array.from([0.5, -1.5, 2.5, 100.25]);
const f64 = Float64Array.from([1.5, 2.25, -3.75, 100.125]);

console.log("i8", sumI8(i8, 8));
console.log("u8", sumU8(u8, 8));
console.log("u8c", sumU8C(u8c, 8));
console.log("i16", sumI16(i16, 8));
console.log("u16", sumU16(u16, 8));
console.log("i32", sumI32(i32, 8));
console.log("u32", sumU32(u32, 8)); // exercises unsigned widening
console.log("f32", sumF32(f32, 4));
console.log("f64", sumF64(f64, 4));

// ---- the bcryptjs `_encipher` shape: `n = S[..]; n += S[..]` with masked ----
// ---- (always in-bounds) integer indices from bitwise ops ----
function feistel(S: Int32Array, x: number): number {
  let n = S[(x >>> 24) & 7];
  n += S[(x >> 16) & 7];
  n ^= S[(x >> 8) & 7];
  n += S[x & 7];
  return n | 0;
}
console.log("feistel", feistel(i32, 0x12345678));

// ---- an in-bounds `1000 + S[i]` (numeric add, real element) ----
function readAdd(S: Int32Array, i: number): number {
  return 1000 + S[i];
}
console.log("inb", readAdd(i32, 3)); // 1000 + 7

// ---- OOB / negative / fractional reads observed in SAFE contexts (the read
// itself yields `undefined`; we avoid `+` here because a separate, pre-existing
// codegen issue mishandles `number + <oob typed-array read>` — tracked apart
// from this fast path, which is bit-exact with the runtime getter). ----
function eqUndef(S: Int32Array, i: number): boolean {
  return S[i] === undefined;
}
function strOf(S: Int32Array, i: number): string {
  return String(S[i]);
}
console.log("oob-eq", eqUndef(i32, 8), eqUndef(i32, -1), eqUndef(i32, 3)); // true true false
console.log("oob-str", strOf(i32, 8), strOf(i32, -1), strOf(i32, 3)); // undefined undefined 7
// Fractional index reads `undefined` (must NOT round to element 3 via ToInt32).
console.log("frac-eq", eqUndef(i32, 3.9), eqUndef(i32, 3)); // true false

// ---- view over an ArrayBuffer (non-inline storage -> slow fallback) ----
function viewSum(S: Int32Array, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s += S[i];
  return s;
}
const ab = new ArrayBuffer(16);
const view = new Int32Array(ab);
view[0] = 111;
view[1] = -222;
view[2] = 333;
view[3] = -444;
console.log("view", viewSum(view, 4)); // -222

// ---- detached buffer: an in-bounds read pre-detach, `=== undefined` post ----
const ab2 = new ArrayBuffer(16);
const det = new Int32Array(ab2);
det[0] = 7;
det[1] = 9;
console.log("predetach", viewSum(det, 2));
ab2.transfer(); // detach
console.log("postdetach", det[0] === undefined, det[1] === undefined); // true true
