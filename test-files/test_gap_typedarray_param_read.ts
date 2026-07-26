// Reading a typed-array element through a *parameter* (erased length / storage).
// perry-codegen expr/i32_fast_path.rs lowers an i32/ToInt32-context read of a
// typed-array PARAM to a checked inline native load (runtime guard: pointer +
// inline-storage PERRY_TA_VIEW_GUARD + kind-cache; header-length bounds check;
// bare load; 0 on in-kind OOB == ToInt32(undefined); slow fallback
// js_typed_array_read_int32 for view/detached/wrong-kind). A plain-value read
// still observes `undefined` OOB. Every line must match
// `node --experimental-strip-types` exactly.

// ---- i32-context (bitwise) reads, one per integer kind, in a loop ----
function xorI32(S: Int32Array, n: number): number {
  let a = 0 | 0;
  for (let i = 0; i < n; i++) a = (a ^ S[i & 7]) | 0;
  return a | 0;
}
function xorI8(S: Int8Array, n: number): number {
  let a = 0 | 0;
  for (let i = 0; i < n; i++) a = (a ^ S[i & 7]) | 0;
  return a | 0;
}
function xorU8(S: Uint8Array, n: number): number {
  let a = 0 | 0;
  for (let i = 0; i < n; i++) a = (a ^ S[i & 7]) | 0;
  return a | 0;
}
function xorU8C(S: Uint8ClampedArray, n: number): number {
  let a = 0 | 0;
  for (let i = 0; i < n; i++) a = (a ^ S[i & 7]) | 0;
  return a | 0;
}
function xorI16(S: Int16Array, n: number): number {
  let a = 0 | 0;
  for (let i = 0; i < n; i++) a = (a ^ S[i & 7]) | 0;
  return a | 0;
}
function xorU16(S: Uint16Array, n: number): number {
  let a = 0 | 0;
  for (let i = 0; i < n; i++) a = (a ^ S[i & 7]) | 0;
  return a | 0;
}
function xorU32(S: Uint32Array, n: number): number {
  let a = 0 | 0;
  for (let i = 0; i < n; i++) a = (a ^ S[i & 7]) | 0;
  return a | 0;
}

const i32 = Int32Array.from([-5, 100000, -2000000000, 7, 0x7fffffff, -1, 42, 999]);
const i8 = Int8Array.from([-5, 100, -128, 7, 127, -1, 42, 99]);
const u8 = Uint8Array.from([1, 200, 255, 7, 128, 0, 42, 99]);
const u8c = Uint8ClampedArray.from([1, 200, 255, 7, 128, 0, 42, 99]);
const i16 = Int16Array.from([-5, 30000, -32768, 7, 32767, -1, 42, 999]);
const u16 = Uint16Array.from([1, 60000, 65535, 7, 32768, 0, 42, 999]);
const u32 = Uint32Array.from([1, 4000000000, 0xffffffff, 7, 0x80000000, 0, 42, 999]);

console.log("i32", xorI32(i32, 8));
console.log("i8", xorI8(i8, 8));
console.log("u8", xorU8(u8, 8));
console.log("u8c", xorU8C(u8c, 8));
console.log("i16", xorI16(i16, 8));
console.log("u16", xorU16(u16, 8));
console.log("u32", xorU32(u32, 8));

// ---- OOB in i32-context: reads past length contribute 0 (ToInt32(undefined)) ----
function xorOob(S: Int32Array): number {
  let a = 12345 | 0;
  for (let i = 0; i < 16; i++) a = (a ^ S[i]) | 0; // i = 8..15 are OOB
  return a | 0;
}
console.log("oob", xorOob(i32));

// A negative & fractional index in i32-context also read as 0.
function readMasked(S: Int32Array, i: number): number {
  return (99 ^ S[i]) | 0;
}
console.log("neg", readMasked(i32, -1)); // S[-1] -> undefined -> 0 -> 99 ^ 0
console.log("frac", readMasked(i32, 3.9)); // fractional -> undefined -> 0
console.log("in", readMasked(i32, 3)); // in-bounds element 7

// ---- plain-value reads: OOB / negative / fractional must be `undefined` ----
function readAt(S: Int32Array, i: number): number | undefined {
  return S[i];
}
console.log("v0", readAt(i32, 0), "v7", readAt(i32, 7));
console.log("voob", readAt(i32, 8)); // undefined
console.log("vneg", readAt(i32, -1)); // undefined
console.log("vfrac", readAt(i32, 1.5)); // undefined
console.log("vstr", String(readAt(i32, 8))); // "undefined"
console.log("veq", readAt(i32, 8) === undefined); // true

// ---- Float64Array param (element width != 4) ----
function sumF64(S: Float64Array, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s += S[i];
  return s;
}
function readF64(S: Float64Array, i: number): number | undefined {
  return S[i];
}
function truncF64(S: Float64Array, i: number): number {
  return S[i] | 0; // i32-context: float -> ToInt32
}
const f64 = Float64Array.from([1.5, 2.25, -3.75, 100.125, 1e12 + 0.5]);
console.log("f64sum", sumF64(f64, 5));
console.log("f64read", readF64(f64, 1), readF64(f64, 10));
console.log("f64trunc", truncF64(f64, 0), truncF64(f64, 2), truncF64(f64, 4), truncF64(f64, 99));

// ---- Float32Array param (width 4, but float kind — stays on runtime read) ----
function readF32(S: Float32Array, i: number): number | undefined {
  return S[i];
}
const f32 = Float32Array.from([0.5, -1.5, 2.5]);
console.log("f32", readF32(f32, 0), readF32(f32, 2), readF32(f32, 9));

// ---- view over an ArrayBuffer (non-inline storage -> slow fallback path) ----
function viewXor(S: Int32Array, n: number): number {
  let a = 0 | 0;
  for (let i = 0; i < n; i++) a = (a ^ S[i]) | 0;
  return a | 0;
}
function viewRead(S: Int32Array, i: number): number | undefined {
  return S[i];
}
const ab = new ArrayBuffer(16);
const view = new Int32Array(ab);
view[0] = 111;
view[1] = -222;
view[2] = 333;
view[3] = -444;
console.log("view", viewXor(view, 4), viewRead(view, 1), viewRead(view, 8));

// ---- detached buffer: reads are undefined (plain) / 0 (i32-context) ----
const ab2 = new ArrayBuffer(16);
const det = new Int32Array(ab2);
det[0] = 7;
det[1] = 9;
console.log("predetach", viewRead(det, 0), viewXor(det, 4));
ab2.transfer(); // detach ab2 (and its view `det`)
console.log("postdetach-plain", viewRead(det, 0)); // undefined
console.log("postdetach-i32", viewXor(det, 4)); // 0 (all OOB after detach)
