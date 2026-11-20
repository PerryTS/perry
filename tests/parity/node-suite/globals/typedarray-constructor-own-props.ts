// #4140 — every concrete TypedArray constructor carries the spec-required own
// properties: `BYTES_PER_ELEMENT` on BOTH the constructor and its prototype
// (`{ value, writable:false, enumerable:false, configurable:false }`) and the
// prototype's `constructor` back-link to the constructor.
//
// The bare `Uint8Array.BYTES_PER_ELEMENT` read folds at compile time (#2902),
// so each constructor is also probed through a runtime `any` binding and the
// reflective protocol (`getOwnPropertyDescriptor`, `hasOwnProperty`, the
// chained `Ctor.prototype.BYTES_PER_ELEMENT` read) which resolve off the real
// installed own properties rather than the fold.

function desc(o: any, k: string): string {
  return JSON.stringify(Object.getOwnPropertyDescriptor(o, k));
}

const sizes: Record<string, number> = {
  Int8Array: 1,
  Uint8Array: 1,
  Uint8ClampedArray: 1,
  Int16Array: 2,
  Uint16Array: 2,
  Float16Array: 2,
  Int32Array: 4,
  Uint32Array: 4,
  Float32Array: 4,
  Float64Array: 8,
  BigInt64Array: 8,
  BigUint64Array: 8,
};

for (const name of Object.keys(sizes)) {
  const C: any = (globalThis as any)[name];
  const expected = sizes[name];
  console.log("== " + name + " ==");
  // Folded constant form and the runtime value form agree with the spec size.
  console.log("ctor BPE", C.BYTES_PER_ELEMENT, C.BYTES_PER_ELEMENT === expected);
  console.log("proto BPE", C.prototype.BYTES_PER_ELEMENT, C.prototype.BYTES_PER_ELEMENT === expected);
  // Descriptors: value + the non-writable/non-enumerable/non-configurable attrs.
  console.log("ctor BPE desc", desc(C, "BYTES_PER_ELEMENT"));
  console.log("proto BPE desc", desc(C.prototype, "BYTES_PER_ELEMENT"));
  // Own-ness via the uncurried hasOwnProperty.
  console.log(
    "hasOwn BPE",
    Object.prototype.hasOwnProperty.call(C, "BYTES_PER_ELEMENT"),
    Object.prototype.hasOwnProperty.call(C.prototype, "BYTES_PER_ELEMENT"),
  );
  // Non-enumerable: must not leak into the constructor's own-keys.
  console.log("ctor keys has BPE", Object.keys(C).includes("BYTES_PER_ELEMENT"));
  // The prototype's `constructor` back-link and its descriptor.
  console.log("proto.constructor === Ctor", C.prototype.constructor === C);
  console.log("constructor desc", desc(C.prototype, "constructor"));
}
