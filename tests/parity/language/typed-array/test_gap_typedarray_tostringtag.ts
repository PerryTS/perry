// %TypedArray%.prototype[@@toStringTag] accessor.
//
// Per ES2024 23.2.3.38, `%TypedArray%.prototype` has a get-only
// `[Symbol.toStringTag]` accessor whose getter returns the typed array's
// constructor name (e.g. "Int8Array") when `this` is a TypedArray, else
// `undefined`. It is `{ enumerable: false, configurable: true, set: undefined }`.
//
// `safe-stable-stringify` (a pino dependency) detects typed arrays via
//   getOwnPropertyDescriptor(%TypedArray%.prototype, Symbol.toStringTag).get
// then `desc.get.call(value)`, so a missing accessor threw
// `Cannot read properties of undefined (reading 'get')`.

const proto = Object.getPrototypeOf(Object.getPrototypeOf(new Int8Array()));
const d = Object.getOwnPropertyDescriptor(proto, Symbol.toStringTag)!;

// 1. The descriptor is a real accessor.
console.log("desc:", typeof d, typeof d.get);
console.log("attrs:", d.enumerable, d.configurable, d.set);

// 2. Per-kind tag via the public symbol — inline and through a variable.
console.log("Int8:", new Int8Array()[Symbol.toStringTag]);
const u8 = new Uint8Array(3);
console.log("Uint8:", u8[Symbol.toStringTag]);
console.log("Uint8Clamped:", new Uint8ClampedArray(2)[Symbol.toStringTag]);
console.log("Int16:", new Int16Array()[Symbol.toStringTag]);
console.log("Uint16:", new Uint16Array()[Symbol.toStringTag]);
console.log("Int32:", new Int32Array()[Symbol.toStringTag]);
console.log("Uint32:", new Uint32Array()[Symbol.toStringTag]);
console.log("Float32:", new Float32Array()[Symbol.toStringTag]);
console.log("Float64:", new Float64Array()[Symbol.toStringTag]);

// 3. `desc.get.call(...)` — the safe-stable-stringify detection idiom.
console.log("get.call(Int8):", d.get!.call(new Int8Array()));
console.log("get.call(Uint8):", d.get!.call(u8));
console.log("get.call({}):", d.get!.call({}));

// 4. No regression to the brand-check `Object.prototype.toString`.
console.log("toString(Int8):", Object.prototype.toString.call(new Int8Array()));
console.log("toString(Uint8):", Object.prototype.toString.call(u8));
console.log("String:", String(new Int8Array(2)));
