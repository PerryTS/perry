// Declared typed-array element reads and writes whose index has no static
// integer proof, and the value conversions of the inline store.

function get(a: Float64Array, k: number): number {
  return a[k];
}
function put(a: Float64Array, k: number, v: any): void {
  a[k] = v;
}
function putI32(a: Int32Array, k: number, v: any): void {
  a[k] = v;
}
function putU32(a: Uint32Array, k: number, v: any): void {
  a[k] = v;
}
function putU8C(a: Uint8ClampedArray, k: number, v: any): void {
  a[k] = v;
}
function putAny(a: any, k: any, v: any): void {
  a[k] = v;
}

const f64 = new Float64Array(8);
for (let i = 0; i < 2000; i++) {
  put(f64, i % 8, i * 0.5);
}
let sum = 0;
for (let i = 0; i < 2000; i++) {
  sum += get(f64, i % 8);
}
console.log("hot", sum, Array.from(f64).join(","));

const keys: number[] = [0, 1.5, -1, -0, NaN, Infinity, 7, 8, 2 ** 32];
for (const k of keys) {
  put(f64, k, 42);
}
console.log("keys", Array.from(f64).join(","), get(f64, 1.5), get(f64, -1), get(f64, 8));

const values: any[] = ["5", true, false, null, undefined, { valueOf: () => 7 }, 3.5, -0, 1e300, -1e300, 2 ** 53 + 1, "x"];
const i32 = new Int32Array(1);
const u32 = new Uint32Array(1);
const u8c = new Uint8ClampedArray(1);
const g64 = new Float64Array(1);
for (let r = 0; r < 500; r++) {
  putI32(i32, 0, r);
  putU32(u32, 0, r);
  put(g64, 0, r);
  putU8C(u8c, 0, r);
}
for (const v of values) {
  putI32(i32, 0, v);
  putU32(u32, 0, v);
  put(g64, 0, v);
  putU8C(u8c, 0, v);
  console.log(
    "value",
    typeof v === "object" && v !== null ? "object" : String(v),
    i32[0],
    u32[0],
    Object.is(g64[0], -0) ? "-0" : g64[0],
    u8c[0],
  );
}

// The erased receiver shares the store arms.
const erased: any = new Int16Array(2);
for (let r = 0; r < 500; r++) putAny(erased, r % 2, r);
putAny(erased, 0, "123");
putAny(erased, 1, 70000.9);
console.log("erased", erased[0], erased[1]);

// Views and a lying annotation stay correct.
const buf = new ArrayBuffer(16);
const view = new Float64Array(buf, 8, 1);
put(view, 0, 9.5);
console.log("view", get(view, 0), new Float64Array(buf)[1]);
const notTyped: any = [1, 2, 3];
put(notTyped, 1, 20);
console.log("array as typed", get(notTyped, 1), notTyped.length);

// The erased-receiver inline store, reached through a function value so the
// call site cannot be folded, with the kind cache warmed by numeric stores
// first. This is the shape that stored NaN-boxed values unconverted.
function storeAny(t: any, i: any, v: any): void {
  t[i] = v;
}
(globalThis as any).__storeAny = storeAny;
const dynStore: any = (globalThis as any)["__store" + "Any"];
const warmF64: any = new Float64Array(4);
const warmI32: any = new Int32Array(4);
const warmU32: any = new Uint32Array(4);
for (let i = 0; i < 3000; i++) {
  dynStore(warmF64, 1, i * 0.25);
  dynStore(warmI32, 1, i);
  dynStore(warmU32, 1, i);
}
for (const v of ["5", true, false, null, undefined, { valueOf: () => 7 }, 1e300, 2 ** 31 + 0.5]) {
  dynStore(warmF64, 1, v);
  dynStore(warmI32, 1, v);
  dynStore(warmU32, 1, v);
  console.log(
    "warm erased",
    typeof v === "object" && v !== null ? "object" : String(v),
    warmF64[1],
    warmI32[1],
    warmU32[1],
  );
}
