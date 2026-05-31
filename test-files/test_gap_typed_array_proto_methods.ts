// #3148: %TypedArray%.prototype remaining methods + instanceof.
// Byte-matched against `node --experimental-strip-types`.
// Receivers are local variables (the acceptance form); inline
// `new Int32Array(...).fill()` is a separate pre-existing limitation
// shared by ALL typed-array methods (including PR #2499's map/filter).

function show(label: string, v: unknown): void {
  console.log(label, JSON.stringify(v));
}

// ---- Int32Array (integer element kind) ----
const fillTA = new Int32Array([1, 2, 3, 4, 5]);
fillTA.fill(7);
show("fill", Array.from(fillTA));

const fillRangeTA = new Int32Array([1, 2, 3, 4, 5]);
fillRangeTA.fill(9, 1, 3);
show("fill-range", Array.from(fillRangeTA));

const cwTA = new Int32Array([1, 2, 3, 4, 5]);
cwTA.copyWithin(0, 3);
show("copyWithin", Array.from(cwTA));

const revTA = new Int32Array([1, 2, 3, 4, 5]);
revTA.reverse();
show("reverse", Array.from(revTA));

const a = new Int32Array([1, 2, 3, 4, 5]);
show("reduce", a.reduce((s, x) => s + x, 0));
show("reduce-noinit", a.reduce((s, x) => s + x));
show("reduceRight", a.reduceRight((acc, x) => acc + "-" + x, "z"));
show("join", a.join("-"));
show("join-default", a.join());
show("indexOf", a.indexOf(3));
show("indexOf-miss", a.indexOf(99));

const liTA = new Int32Array([1, 3, 3, 4]);
show("lastIndexOf", liTA.lastIndexOf(3));
show("includes", a.includes(3));
show("includes-miss", a.includes(99));

const sl = a.slice(1, 3);
show("slice", Array.from(sl));
show("slice-same-kind", sl instanceof Int32Array);

const sub = a.subarray(1, 3);
show("subarray", Array.from(sub));
show("subarray-same-kind", sub instanceof Int32Array);

const setTarget = new Int32Array([0, 0, 0, 0]);
setTarget.set([9, 9], 1);
show("set", Array.from(setTarget));

const keys: number[] = [];
for (const k of a.keys()) keys.push(k);
show("keys", keys);

const vals: number[] = [];
for (const v of a.values()) vals.push(v);
show("values", vals);

const ents: Array<[number, number]> = [];
for (const [i, v] of a.entries()) ents.push([i, v]);
show("entries", ents);

// ---- instanceof ----
show("instanceof-self", a instanceof Int32Array);
show("instanceof-wrong", a instanceof Float64Array);
show("instanceof-nonTA", [1, 2, 3] instanceof Int32Array);

// ---- Float64Array (float element kind) ----
const f = new Float64Array([1.5, 2.5, 3.5]);
const f64fill = new Float64Array([1, 2, 3]);
f64fill.fill(0.25);
show("f64-fill", Array.from(f64fill));
show("f64-reduce", f.reduce((s, x) => s + x, 0));
show("f64-join", f.join(","));
show("f64-indexOf", f.indexOf(2.5));

const nanTA = new Float64Array([NaN]);
show("f64-includes-NaN", nanTA.includes(NaN));
show("f64-slice-kind", f.slice(0, 1) instanceof Float64Array);
show("f64-instanceof", f instanceof Float64Array);
