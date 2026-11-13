// for-of over a source annotated as a numeric typed array walks it by index
// only when the value really is one (a runtime admission); any other value is
// iterated by its own iterator, or rejected with node's TypeError when it is
// not iterable. Before, every value behind the annotation was walked by index:
// a Number or an array-like object iterated 0 times / as an array-like, a Set
// or a generator object 0 times.
function mk(k: string): any {
  if (k === "null") return JSON.parse("null");
  if (k === "undef") return JSON.parse("[]")[0];
  if (k === "num") return JSON.parse("3");
  if (k === "obj") return JSON.parse('{"0":5,"length":1}');
  if (k === "arr") return [7, 8];
  if (k === "str") return "ab";
  if (k === "f64") return new Float64Array([1.5, 2.5]);
  if (k === "i32") return new Int32Array([-3, 4]);
  if (k === "u8") return new Uint8Array([200, 1]);
  if (k === "u8c") return new Uint8ClampedArray([255, 0]);
  if (k === "f32") return new Float32Array([0.5, -2]);
  if (k === "big") return new BigInt64Array([5n, -6n]);
  if (k === "set") return new Set([1, 2]);
  if (k === "map") return new Map([[1, 2]]);
  if (k === "it") return { *[Symbol.iterator]() { yield 4; yield 5; } };
  return 0;
}
function f64(a: Float64Array): string { const o: any[] = []; for (const x of a) o.push(x); return o.join(","); }
function i32(a: Int32Array): string { const o: any[] = []; for (const x of a) o.push(x); return o.join(","); }
function u8(a: Uint8Array): string { const o: any[] = []; for (const x of a) { o.push(x); } return o.join(","); }
function brk(a: Float64Array): string { const o: any[] = []; for (const x of a) { o.push(x); break; } return o.join(","); }
function mut(a: Int32Array): string {
  const o: any[] = [];
  let i = 0;
  for (const x of a) {
    o.push(x);
    if (i++ === 0 && typeof a === "object" && a.length > 1) (a as any)[1] = typeof x === "bigint" ? 9n : 9;
  }
  return o.join(",");
}
function sum(a: Float64Array): number { let s = 0; for (const x of a) s += x; return s; }
function local(v: any): string { const a: Uint8Array = v; const o: any[] = []; for (const x of a) o.push(x); return o.join(","); }
// Heads the index rewrite does not take: the holder is the value when it is a
// typed array, else what its iterator yields.
function pat(a: Float64Array): string { const o: any[] = []; for (const [x] of a) o.push(x); return o.join(","); }
function vr(a: Uint16Array): string { const o: any[] = []; for (var x of a) o.push(x); return o.join(","); }
function show(f: () => any): string {
  try { return "[" + String(f()) + "]"; } catch (e: any) { return "T:" + e.name; }
}
for (const k of ["null", "undef", "num", "obj", "arr", "str", "f64", "i32", "u8", "u8c", "f32", "big", "set", "map", "it"]) {
  console.log(k, show(() => f64(mk(k))), show(() => i32(mk(k))), show(() => u8(mk(k))), show(() => brk(mk(k))),
    show(() => mut(mk(k))), show(() => local(mk(k))), show(() => vr(mk(k))));
}
for (const k of ["num", "map", "arr", "f64"]) console.log("pat", k, show(() => pat(mk(k))));
console.log("sum", sum(new Float64Array([1, 2, 3.5])), show(() => sum(mk("set"))), show(() => sum(mk("num"))));

// A generator behind the annotation runs lazily and is closed by a break.
const log: string[] = [];
function* gen() {
  try { log.push("y1"); yield 1; log.push("y2"); yield 2; } finally { log.push("closed"); }
}
function first(a: Float64Array): number { for (const x of a) { log.push("body " + x); return x; } return -1; }
console.log("gen", first(gen() as any), log.join(" "));

// Module-level loops take the same admission.
const top1: Float64Array = mk("set");
const outTop: any[] = [];
for (const x of top1) outTop.push(x);
const top2: Int8Array = new Int8Array([-1, 2]);
for (const x of top2) outTop.push(x * 2);
console.log("top", outTop.join(","));
try {
  const top3: Uint8Array = mk("num");
  for (const x of top3) outTop.push(x);
  console.log("top3 no throw");
} catch (e: any) {
  console.log("top3", e.name);
}
