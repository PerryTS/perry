// Regression for the `Object.defineProperty` attribute-retention bug that
// blocked zod v4 under `perry.compilePackages` (discussion #3438).
//
// Spec (OrdinaryDefineOwnProperty / ValidateAndApplyPropertyDescriptor):
// when redefining an EXISTING own property, any attribute the descriptor
// omits must RETAIN the property's current value — it does NOT reset to the
// new-property `false` default. Perry previously reset omitted attributes to
// `false` on every define, so converting an enumerable data property to a
// getter (and back to a value) silently dropped its enumerability.
//
// zod's `$ZodObject` relies on this: it replaces the enumerable `shape` data
// property with a self-replacing getter via `Object.defineProperty`, then
// `{ ...def }` is expected to still spread `shape`. Pre-fix `shape` became
// non-enumerable, `{ ...def }.shape` was undefined, and object parsing crashed
// with "Cannot read properties of undefined (reading '_zod')".
//
// Output must match `node --experimental-strip-types` byte-for-byte.

// 1. Redefine an existing enumerable data property with only { value } —
//    enumerable must stay true.
const a: any = { x: 1 };
Object.defineProperty(a, "x", { value: 2 });
console.log("redefine value keeps enumerable:", Object.keys(a).includes("x"));

// 2. Convert an enumerable data property to a getter (only { get }) —
//    enumerable retained, then back to a value — still retained.
const b: any = { y: 1 };
Object.defineProperty(b, "y", { get: () => 99 });
console.log("data->getter keeps enumerable:", Object.keys(b).includes("y"));
Object.defineProperty(b, "y", { value: 42 });
console.log("getter->value keeps enumerable:", Object.keys(b).includes("y"));
console.log("spread keeps it:", "y" in { ...b });

// 3. Configurable is likewise retained: a configurable property stays
//    configurable across a value-only redefine (no "Cannot redefine" throw).
const c: any = { z: 1 };
Object.defineProperty(c, "z", { value: 2 });
Object.defineProperty(c, "z", { value: 3 });
console.log("still configurable z:", c.z);

// 4. Control: a BRAND-NEW property defined via defineProperty still defaults
//    to non-enumerable (omitted attributes are false for new properties).
const d: any = {};
Object.defineProperty(d, "n", { value: 7 });
console.log("new prop non-enumerable:", Object.keys(d).includes("n"));

// 5. The zod-shaped pattern: enumerable data prop -> self-replacing getter ->
//    value, then enumerable spread must still see it.
const def: any = { type: "object", shape: { a: 1, b: 2 } };
const sh = def.shape;
Object.defineProperty(def, "shape", {
  get: () => {
    const newSh = { ...sh };
    Object.defineProperty(def, "shape", { value: newSh });
    return newSh;
  },
});
const keys = Object.keys(def.shape); // triggers the getter
const spread = { ...def };
console.log("normalized keys:", keys.join(","));
console.log("spread.shape defined:", spread.shape !== undefined);
console.log("spread.shape keys:", Object.keys(spread.shape).join(","));
