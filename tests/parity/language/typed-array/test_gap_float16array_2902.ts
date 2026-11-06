// Gap test: Float16Array (#2902)
// Run: node --experimental-strip-types test_gap_float16array_2902.ts

// --- typeof / constructor identity ---
console.log("typeof:", typeof Float16Array);
console.log("BYTES static:", Float16Array.BYTES_PER_ELEMENT);

// --- construct from array (exactly-representable f16 values) ---
const a = new Float16Array([1, 1.5, 2, 0.5, -3]);
console.log("from array length:", a.length);
console.log("from array values:", a[0], a[1], a[2], a[3], a[4]);
console.log("BYTES instance:", a.BYTES_PER_ELEMENT);
console.log("byteLength:", a.byteLength);

// --- construct from length ---
const b = new Float16Array(4);
console.log("length ctor length:", b.length);
console.log("length ctor zero:", b[0], b[1], b[2], b[3]);

// --- indexed write/read with f16 rounding ---
const c = new Float16Array(3);
c[0] = 1.5;
c[1] = 0.5;
c[2] = -2;
console.log("write/read:", c[0], c[1], c[2]);

// 65504 is the max finite half; exactly representable.
c[0] = 65504;
console.log("max half:", c[0]);
// Overflow → Infinity.
c[1] = 70000;
console.log("overflow:", c[1]);
// Tiny value underflows toward 0 (1e-8 → 0).
c[2] = 1e-8;
console.log("underflow:", c[2]);

// --- instanceof ---
console.log("instanceof:", a instanceof Float16Array);

// --- Float16Array.from ---
const d = Float16Array.from([2, 4, 8]);
console.log("from:", d[0], d[1], d[2], "len", d.length);

// --- Float16Array.of ---
const e = Float16Array.of(1, 2, 3);
console.log("of:", e[0], e[1], e[2], "len", e.length);

// --- spread / iteration ---
const arr = [...a];
console.log("spread:", arr.join(","));

let sum = 0;
for (const v of a) {
  sum += v;
}
console.log("for-of sum:", sum);

// NOTE: ArrayBuffer-view construction (`new Float16Array(buffer, off, len)`)
// is a pre-existing Perry gap that affects ALL typed arrays (e.g.
// `new Float64Array(new ArrayBuffer(8)).length` is also wrong today), so it
// is intentionally out of scope for this Float16Array issue and not asserted
// here. Tracked separately.
