// %TypedArray%.prototype.fill: value converted once, before start/end; spec clamping;
// fill in the array's own element kind. Output must equal node.
const order: string[] = [];
function r(fn: () => any): string {
  try { const v = fn(); return "ok " + String(v); } catch (e: any) { return e.constructor.name; }
}
function show(a: any): string { return Array.from(a as any, (x: any) => Object.is(x, -0) ? "-0" : String(x)).join(","); }
const ctors: any[] = [Int8Array, Uint8Array, Uint8ClampedArray, Int16Array, Uint16Array, Int32Array, Uint32Array, Float32Array, Float64Array];
const vals: any[] = [0, -0, 1, -1, 1.5, 2.5, 0.5, 254.5, 255.5, 300, -300, 70000, 2 ** 31, 2 ** 32 + 5, -(2 ** 31) - 1,
  NaN, Infinity, -Infinity, 1e21, "7", "x", null, undefined, true, false, [], [3]];
for (const C of ctors) {
  for (const v of vals) console.log(C.name, String(v), show(new C(4).fill(v)));
  console.log(C.name, "-0", show(new C(2).fill(-0)));
}
// ranges
const ranges: any[][] = [[], [1], [1, 3], [-2], [-3, -1], [1.9, 3.9], [-1.5], [NaN, NaN], [undefined, undefined], [null, null],
  [Infinity], [-Infinity, Infinity], [2, 1], [5, 9], [-9, 9], ["1", "3"], [0, undefined], [0, null], [0, -0], [1, Infinity], [-Infinity, 2], [0, NaN]];
for (const rg of ranges) console.log("range", JSON.stringify(rg.map(x => String(x))), show(new Int16Array(5).fill(7, ...rg)));
// returns this
const t = new Float64Array(3); console.log("this", t.fill(1) === t, t.fill(2, 1) === t);
// subarray view / byteOffset
const buf = new ArrayBuffer(16); const full = new Uint8Array(buf);
const sub = new Uint8Array(buf, 4, 6); sub.fill(9, 1, 5); console.log("sub", show(full));
const f32 = new Float32Array(buf, 8, 2); f32.fill(1.5); console.log("f32view", show(full));
const sa = new Int16Array([1, 2, 3, 4, 5, 6]).subarray(2, 5); sa.fill(-1, 1); console.log("subarray", show(sa));
// valueOf order: value first, then start, then end, each once
const log: string[] = [];
const mk = (n: string, v: any) => ({ valueOf() { log.push(n); return v; } });
new Float64Array(4).fill(mk("v", 1) as any, mk("s", 1) as any, mk("e", 3) as any);
console.log("order", log.join(","));
// value conversion once for a long array
let calls = 0; const z = new Uint8Array(1000).fill({ valueOf() { calls++; return 3; } } as any); console.log("once", calls, z[999]);
// value valueOf throws -> start/end untouched
const log2: string[] = [];
console.log("throw", r(() => new Int8Array(3).fill({ valueOf() { throw new RangeError("boom"); } } as any, mk("s2", 0) as any)), log2.length, log.length);
// Uint8Clamped rounding
const uc = new Uint8ClampedArray(1); for (const v of [0.5, 1.5, 2.5, 3.5, 254.5, 255, 255.4, -0.5, 0.49999, 0.50001]) { uc.fill(v); console.log("clamp", v, uc[0]); }
// BigInt kinds
for (const C of [BigInt64Array, BigUint64Array] as any[]) {
  for (const v of [0n, 1n, -1n, 2n ** 63n, 2n ** 64n + 3n, -(2n ** 63n) - 1n, true, "12", false]) console.log(C.name, String(v), r(() => new C(3).fill(v)), show(new C(3).fill(v)));
  for (const v of [1, 1.5, NaN, undefined, null, Symbol.iterator, {}, "x", "1.5"]) console.log(C.name, "bad", typeof v, r(() => new C(2).fill(v)));
  console.log(C.name, "range", show(new C(5).fill(7n, 1, -1)));
  const lg: string[] = [];
  console.log(C.name, "obj", r(() => show(new C(2).fill({ valueOf() { lg.push("v"); return 5n; } } as any, mk("s", 0) as any))), lg.join(","));
}
// Number into float kinds with non-number values: Symbol
for (const C of ctors) console.log(C.name, "symbol", r(() => new C(1).fill(Symbol("s") as any)));
console.log("bigint to number kind", r(() => new Int8Array(1).fill(1n as any)));
// empty
console.log("empty", show(new Int8Array(0).fill(5)), r(() => new Float32Array(0).fill(1, 5, 9)));
// detached
const db = new ArrayBuffer(8); const du = new Uint8Array(db);
(db as any).transfer?.();
console.log("detached", r(() => du.fill(1)));
const db2 = new ArrayBuffer(8); const du2 = new Int32Array(db2);
console.log("detached-in-valueOf", r(() => du2.fill({ valueOf() { (db2 as any).transfer(); return 1; } } as any)));
const db3 = new ArrayBuffer(8); const du3 = new Int32Array(db3);
console.log("detached-in-start", r(() => du3.fill(1, { valueOf() { (db3 as any).transfer(); return 0; } } as any)));
// resizable buffer shrinking during conversion
try {
  const rb = new (ArrayBuffer as any)(8, { maxByteLength: 16 }); const ru = new Uint8Array(rb); ru.fill(1);
  console.log("resize-shrink", r(() => ru.fill({ valueOf() { rb.resize(4); return 9; } } as any)), show(ru));
  const rb2 = new (ArrayBuffer as any)(8, { maxByteLength: 16 }); const ru2 = new Uint8Array(rb2);
  console.log("resize-in-end", r(() => ru2.fill(5, 0, { valueOf() { rb2.resize(3); return 8; } } as any)), show(ru2));
  const rb3 = new (ArrayBuffer as any)(8, { maxByteLength: 16 }); const ru3 = new Uint8Array(rb3, 0, 8);
  console.log("resize-oob", r(() => ru3.fill({ valueOf() { rb3.resize(4); return 9; } } as any)));
} catch (e: any) { console.log("resize unsupported", e.constructor.name); }
// receiver not a typed array
console.log("recv", r(() => (Int8Array.prototype.fill as any).call([1, 2], 1)), r(() => (Int8Array.prototype.fill as any).call({}, 1)));
// frozen
console.log("big", (() => { const a = new Float32Array(43200).fill(0.25); let s = 0; for (let i = 0; i < a.length; i++) s += a[i]; return s; })());
