// #4102 — dynamic `instanceof` and reflective `@@hasInstance` with the
// `Array` / `Object` / `Date` constructor *values*. The literal-RHS operator
// (`[] instanceof Array`) already resolves these at compile time; the dynamic
// form (`[] instanceof A` where `A = Array`) and the reflective
// `C[Symbol.hasInstance](v)` form previously returned `false` / `undefined`.
// Output is booleans so it is byte-identical to Node.

const A = Array;
const O = Object;
const D = Date;

// Dynamic `instanceof` against a constructor held in a variable.
console.log("arr A:", ([] as any) instanceof A); // true
console.log("obj O:", ({} as any) instanceof O); // true
console.log("date D:", (new Date() as any) instanceof D); // true

// Cross checks: arrays/dates are objects; plain objects are not arrays.
console.log("arr O:", ([] as any) instanceof O); // true
console.log("date O:", (new Date() as any) instanceof O); // true
console.log("obj A:", ({} as any) instanceof A); // false
console.log("date A:", (new Date() as any) instanceof A); // false

// Direct `C[Symbol.hasInstance](v)` for the built-in constructor values.
console.log("Array hi:", (Array as any)[Symbol.hasInstance]([])); // true
console.log("Object hi:", (Object as any)[Symbol.hasInstance]({})); // true
console.log("Date hi:", (Date as any)[Symbol.hasInstance](new Date())); // true
console.log("Array hi obj:", (Array as any)[Symbol.hasInstance]({})); // false

// The `Function.prototype[Symbol.hasInstance].call(C, v)` form (already worked
// via #4098) stays consistent with the direct read above.
const hi = Function.prototype[Symbol.hasInstance];
console.log("call Array:", hi.call(Array, [])); // true
console.log("call Date:", hi.call(Date, new Date())); // true
