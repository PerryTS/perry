// Issue #2061: non-index (method-name) keys on a typed array must resolve
// against %TypedArray%.prototype instead of being swallowed by the element
// fast path (which returned a number, so calls threw "is not a function").
//
// This exercises the computed-key read + dynamic-dispatch call forms the
// issue documents. NOTE: the *static* call form `arr.map(cb)` is lowered by
// the HIR into dedicated Array* nodes (array layout) — that reification is a
// separate gap (#793), so it's intentionally not asserted here.

// --- the exact repro: every prototype method reads as a function ---
const a = new Int8Array([1, 2, 3, 4]);
for (const m of ["copyWithin", "fill", "subarray", "slice", "set", "indexOf", "map", "filter", "reduce", "join"]) {
  console.log("typeof", m, typeof (a as any)[m]);
}
console.log("len/elem", a.length, a[0]);

// --- static member-as-value form resolves too ---
console.log("static typeof", typeof a.map, typeof a.fill, typeof a.reduce);

// --- accessor properties ---
const u16 = new Uint16Array([10, 20, 30]);
console.log("byteLength", u16.byteLength, "BPE", u16.BYTES_PER_ELEMENT, "byteOffset", u16.byteOffset, "length", u16.length);

// --- dynamic-key method calls dispatch to the prototype impl ---
const f = new Float64Array([5, 3, 1, 4, 2]);
for (const k of ["indexOf"]) console.log("indexOf", (f as any)[k](4), (f as any)[k](99));
for (const k of ["lastIndexOf"]) console.log("lastIndexOf", (new Int8Array([1, 2, 1, 2]) as any)[k](1));
for (const k of ["includes"]) console.log("includes", (f as any)[k](3), (f as any)[k](99));
for (const k of ["join"]) console.log("join", (f as any)[k]("-"));
for (const k of ["slice"]) console.log("slice", Array.from((f as any)[k](1, 3)));
for (const k of ["subarray"]) console.log("subarray", Array.from((f as any)[k](2)));
for (const k of ["map"]) console.log("map", Array.from((f as any)[k]((x: number) => x * 2)));
for (const k of ["filter"]) console.log("filter", Array.from((f as any)[k]((x: number) => x > 2)));
for (const k of ["reduce"]) console.log("reduce", (f as any)[k]((p: number, c: number) => p + c, 0));
for (const k of ["reduceRight"]) console.log("reduceRight", (new Int32Array([1, 2, 3]) as any)[k]((p: number, c: number) => p - c));
for (const k of ["some"]) console.log("some", (f as any)[k]((x: number) => x > 4));
for (const k of ["every"]) console.log("every", (f as any)[k]((x: number) => x > 0));
for (const k of ["find"]) console.log("find", (f as any)[k]((x: number) => x > 3));
for (const k of ["findIndex"]) console.log("findIndex", (f as any)[k]((x: number) => x > 3));

// --- in-place mutators via dynamic dispatch ---
const fill = new Int16Array(4);
for (const k of ["fill"]) (fill as any)[k](9, 1, 3);
console.log("fill", Array.from(fill));

const cw = new Int8Array([1, 2, 3, 4, 5]);
for (const k of ["copyWithin"]) (cw as any)[k](0, 3);
console.log("copyWithin", Array.from(cw));

const rev = new Int8Array([1, 2, 3]);
for (const k of ["reverse"]) (rev as any)[k]();
console.log("reverse", Array.from(rev));

// (Int16Array — not a Buffer-backed Uint8Array, which routes through the
// separate Buffer dispatch path.)
const dst = new Int16Array(5);
for (const k of ["set"]) (dst as any)[k]([10, 20, 30], 1);
console.log("set", Array.from(dst));
