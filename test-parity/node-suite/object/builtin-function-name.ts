// #2144: a built-in function's `name` own-property read back `undefined`
// (and `getOwnPropertyDescriptor(Promise.race, "name")` returned undefined).
// Per spec, `name` is a `{ writable:false, enumerable:false, configurable:true }`
// own data property whose value is the function's name. Follow-up to #2075
// (#2059), which only covered user-defined function declarations and
// expressions.
//
// Descriptor fields are logged individually so the byte-for-byte comparison
// doesn't depend on console's multi-line object formatting.

// Built-in constructors.
console.log("Promise:", Promise.name);
console.log("Array:", Array.name);
console.log("Object:", Object.name);
console.log("Map:", Map.name);
console.log("Set:", Set.name);

// Built-in Error constructors — `assert.throws` reports
// `expectedErrorConstructor.name` to label thrown errors.
console.log("Error:", Error.name);
console.log("TypeError:", TypeError.name);
console.log("RangeError:", RangeError.name);
console.log("SyntaxError:", SyntaxError.name);
console.log("ReferenceError:", ReferenceError.name);

// Static functions on built-in namespaces / constructors.
console.log("Math.min:", Math.min.name);
console.log("Math.max:", Math.max.name);
console.log("Math.floor:", Math.floor.name);
console.log("Math.abs:", Math.abs.name);
console.log("Promise.race:", Promise.race.name);
console.log("Promise.all:", Promise.all.name);
console.log("Promise.resolve:", Promise.resolve.name);
console.log("Array.isArray:", Array.isArray.name);
console.log("Array.from:", Array.from.name);
console.log("Object.keys:", Object.keys.name);
console.log("Object.assign:", Object.assign.name);
console.log("Number.isFinite:", Number.isFinite.name);
console.log("Number.isInteger:", Number.isInteger.name);
console.log("JSON.parse:", JSON.parse.name);
console.log("JSON.stringify:", JSON.stringify.name);

// Descriptor form — Test262 inspects the data descriptor directly.
const d1 = Object.getOwnPropertyDescriptor(Promise.race, "name");
console.log(
  "Promise.race desc:",
  d1?.value,
  d1?.writable,
  d1?.enumerable,
  d1?.configurable,
);
const d2 = Object.getOwnPropertyDescriptor(TypeError, "name");
console.log(
  "TypeError desc:",
  d2?.value,
  d2?.writable,
  d2?.enumerable,
  d2?.configurable,
);
const d3 = Object.getOwnPropertyDescriptor(Math.min, "name");
console.log(
  "Math.min desc:",
  d3?.value,
  d3?.writable,
  d3?.enumerable,
  d3?.configurable,
);

// Non-function members must NOT fold (Math.PI is a number — `.name` is
// undefined per spec, not the string "PI").
console.log("Math.PI.name:", (Math.PI as any).name);

// Local shadowing should bypass the fold and fall through to the lookup
// on the user value. The arrow stored via object-literal property syntax
// is left to the runtime — testing here only ensures we do NOT mis-fold.
{
  const Math: any = { custom: () => "shadowed" };
  console.log("shadowed has-prop:", typeof Math.custom);
}
