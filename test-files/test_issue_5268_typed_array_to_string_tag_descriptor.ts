// #5268 (pino follow-up wall): `%TypedArray%.prototype[Symbol.toStringTag]`
// must be an accessor whose `.get` is callable — not an absent (`undefined`)
// descriptor.
//
// After #5269 cleared the `Object prototype may only be an Object or null:
// undefined` throw, pino's next wall came from `safe-stable-stringify`:
//
//   const typedArrayPrototypeGetSymbolToStringTag =
//     Object.getOwnPropertyDescriptor(
//       Object.getPrototypeOf(Object.getPrototypeOf(new Int8Array())),
//       Symbol.toStringTag
//     ).get
//
// Perry returned `undefined` for that descriptor, so `.get` threw
// `TypeError: Cannot read properties of undefined (reading 'get')`. The fix
// installs the spec accessor (ES2024 23.2.3.38) on the shared
// `%TypedArray%.prototype`: a non-enumerable, configurable getter that returns
// the receiver's `[[TypedArrayName]]` for a typed-array receiver and
// `undefined` (no throw) for anything else.

const taProto = Object.getPrototypeOf(Object.getPrototypeOf(new Int8Array()));
const desc = Object.getOwnPropertyDescriptor(taProto, Symbol.toStringTag);

console.log("desc is object:", typeof desc === "object" && desc !== null);
console.log("get is function:", typeof (desc as any).get === "function");
console.log("set is undefined:", (desc as any).set === undefined);
console.log("enumerable:", (desc as any).enumerable);
console.log("configurable:", (desc as any).configurable);

const getter = (desc as any).get as Function;
console.log("tag for Int8Array:", getter.call(new Int8Array()));
console.log("tag for Float64Array:", getter.call(new Float64Array()));
console.log("tag for plain object:", getter.call({}));

console.log("Object.prototype.toString:", Object.prototype.toString.call(new Uint8Array()));
