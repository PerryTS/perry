// #4140 — the `%TypedArray%.prototype` accessor getters
// (`buffer`/`byteLength`/`byteOffset`/`length`) must throw a `TypeError` when
// read off the prototype object instead of a concrete typed-array instance.
// The prototype is an ordinary object with no `[[ViewedArrayBuffer]]` slot
// (test262 `built-ins/TypedArrayConstructors/<T>/prototype/not-typedarray-object.js`),
// so Node throws there. Perry previously returned `undefined`: the per-kind
// prototypes resolve `[[Prototype]]` to the shared `%TypedArray%.prototype`
// only through `Object.getPrototypeOf`, so a plain value read never reached the
// inherited accessor.

const accessors = ["buffer", "byteLength", "byteOffset", "length"] as const;

const types = [
  "Int8Array",
  "Uint8Array",
  "Uint8ClampedArray",
  "Int16Array",
  "Uint16Array",
  "Int32Array",
  "Uint32Array",
  "Float32Array",
  "Float64Array",
  "BigInt64Array",
  "BigUint64Array",
];

function reads(o: any, k: string): string {
  try {
    const v = o[k];
    return "value:" + String(v);
  } catch (e: any) {
    return "throws:" + e.constructor.name;
  }
}

for (const name of types) {
  const C: any = (globalThis as any)[name];
  console.log("== " + name + " ==");
  for (const k of accessors) {
    // Off the per-kind prototype: must throw TypeError.
    console.log("proto." + k, reads(C.prototype, k));
  }
}

// The shared %TypedArray%.prototype intrinsic also throws for these reads.
const TAP: any = Object.getPrototypeOf(Uint8Array.prototype);
console.log("== %TypedArray%.prototype ==");
for (const k of accessors) {
  console.log("intrinsic." + k, reads(TAP, k));
}

// Real instances keep working — the accessors return their concrete values.
const inst = new Uint8Array(4);
console.log("== instance ==");
console.log("byteLength", inst.byteLength);
console.log("byteOffset", inst.byteOffset);
console.log("length", inst.length);
console.log("buffer is object", typeof inst.buffer);

// An object that merely inherits from a typed-array prototype is still not a
// typed array, so the inherited accessor throws there too.
const derived: any = Object.create(Uint8Array.prototype);
console.log("== Object.create(Uint8Array.prototype) ==");
for (const k of accessors) {
  console.log("derived." + k, reads(derived, k));
}

// Reflection is unaffected: the getter is still discoverable on the intrinsic.
console.log(
  "intrinsic buffer has getter",
  typeof Object.getOwnPropertyDescriptor(TAP, "buffer")?.get === "function",
);
