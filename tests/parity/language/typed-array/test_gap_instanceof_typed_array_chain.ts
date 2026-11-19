// `instanceof` a typed-array constructor follows the value's actual chain
// and consults an own @@hasInstance.

// instanceof over typed-array kinds, arrays, records, primitives.
const values: unknown[] = [
  new Float32Array(1), new Float64Array(1), new Int8Array(1), new Uint8Array(1),
  [1, 2], {}, { length: 1 }, 1, "s", null, undefined, Object.create(Float32Array.prototype),
];
for (const v of values) {
  console.log(
    "instanceof",
    v instanceof Float32Array, v instanceof Float64Array, v instanceof Int8Array,
    v instanceof Uint8Array,
  );
}

// A relinked kind prototype: the chain decides, not the element kind.
const i8 = new Int8Array(2);
Object.setPrototypeOf(Int8Array.prototype, Float64Array.prototype);
console.log("relinked kind", i8 instanceof Float64Array, i8 instanceof Int8Array, i8 instanceof Float32Array);
Object.setPrototypeOf(Int8Array.prototype, Object.getPrototypeOf(Float32Array.prototype));
console.log("restored kind", i8 instanceof Float64Array, i8 instanceof Int8Array);

// An own @@hasInstance on a typed-array constructor is consulted.
Object.defineProperty(Uint16Array, Symbol.hasInstance, { value: (v: unknown) => v === 42, configurable: true });
console.log("hasInstance", (42 as unknown) instanceof Uint16Array, new Uint16Array(1) instanceof Uint16Array);
delete (Uint16Array as any)[Symbol.hasInstance];
console.log("hasInstance removed", new Uint16Array(1) instanceof Uint16Array);
