// `ta.set(src)` reached through an untyped receiver: the call resolves `set`
// through the receiver's own properties and prototype chain on every call,
// and the element copy keeps SetTypedArrayFromArrayLike's per-element order.
const any = (v: unknown): any => v;
const f32: any = any(new Float32Array(4));
f32.set([1.5, 2.5, 3.5, 4.5]);
console.log("plain", Array.from(f32).join(","));
f32.set([9, 8], 2);
console.log("offset", Array.from(f32).join(","));

// Holes, strings and objects with valueOf take the coercing path in order.
const order: string[] = [];
const src: any[] = [1, "2", { valueOf() { order.push("v"); return 3; } }];
src[5] = 6;
const f64: any = any(new Float64Array(6));
f64.set(src);
console.log("mixed", Array.from(f64).join(","), order.join(""));
// A throwing coercion leaves the earlier elements written.
const i32: any = any(new Int32Array(3));
try {
  i32.set([7, 8, { valueOf() { throw new Error("boom"); } }]);
} catch (e) {
  console.log("throw", (e as Error).message, Array.from(i32).join(","));
}

// Typed-array sources: same kind, other kinds, overlap within one buffer.
const buf = new ArrayBuffer(32);
const a8 = new Uint8Array(buf, 0, 8);
for (let i = 0; i < 8; i++) a8[i] = i + 1;
any(new Uint8Array(buf, 2, 8)).set(a8);
console.log("overlap-same", Array.from(new Uint8Array(buf, 0, 10)).join(","));
const asF32 = new Float32Array(buf, 0, 4);
const asU16 = new Uint16Array(buf, 4, 4);
asU16.set([1, 2, 3, 4]);
any(asF32).set(asU16);
console.log("overlap-kind", Array.from(asF32).join(","));
const clamp: any = any(new Uint8ClampedArray(3));
clamp.set(new Float64Array([-5, 1.5, 300]));
console.log("clamped", Array.from(clamp).join(","));

// An own `set` shadows the builtin.
const own: any = any(new Float32Array(2));
own.set = (x: unknown) => console.log("own set", JSON.stringify(x));
own.set([1, 2]);
// Out of range throws RangeError.
try {
  any(new Float32Array(2)).set([1, 2, 3]);
} catch (e) {
  console.log("range", (e as Error).constructor.name);
}
