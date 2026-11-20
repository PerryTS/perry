// `v instanceof <TypedArray constructor>` follows the value's actual chain and
// the constructor's @@hasInstance.
const vals: unknown[] = [new Float32Array(2), new Int8Array(2), new Uint8Array(2), [1], {}, 1, null];
const ctors: any[] = [Float32Array, Int8Array, Uint8Array, Float64Array];
for (const c of ctors) console.log(c.name, vals.map((v) => (v as any) instanceof c).join(","));

class Sub extends Float32Array {}
const s = new Sub(2);
console.log("sub", s instanceof Sub, s instanceof Float32Array, s instanceof Int8Array);

const f = new Float32Array(2);
Object.setPrototypeOf(f, Int8Array.prototype);
console.log("relinked", f instanceof Float32Array, f instanceof Int8Array);

Object.defineProperty(Float32Array, Symbol.hasInstance, { value: (v: unknown) => v === 42 });
console.log("hook", (42 as any) instanceof Float32Array, new Float32Array(1) instanceof Float32Array);
const TA = Object.getPrototypeOf(Int16Array);
Object.defineProperty(TA, Symbol.hasInstance, { value: () => true });
console.log("parent hook", ({} as any) instanceof Int16Array, ({} as any) instanceof Float32Array);
