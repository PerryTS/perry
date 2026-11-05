// Issue #3149 (Bug 1) — `Object(value)` called as a plain function (not `new`).
// Per ECMAScript §20.1.1.1, `Object()`/`Object(undefined)`/`Object(null)`
// yield a fresh ordinary `{}`; an existing object/array/function passes
// through unchanged; a primitive coerces to an object (so `typeof` is
// "object"). Pre-fix the bare-call path fell through to the generic
// dispatcher and returned `undefined`. All lines compare byte-for-byte
// against `node --experimental-strip-types`.

// Nullish / no-arg → fresh object.
console.log(typeof Object(null)); // object
console.log(typeof Object()); // object
console.log(typeof Object(undefined)); // object

// The fresh object is a real, mutable ordinary object.
const o = Object(null);
o.x = 5;
console.log(o.x); // 5

// Existing object / array pass through by identity.
const a = [1, 2, 3];
console.log(Object(a) === a); // true
const ob = { k: 1 };
console.log(Object(ob) === ob); // true

// Primitives coerce to objects (typeof "object").
console.log(typeof Object(5)); // object
console.log(typeof Object("hi")); // object
console.log(typeof Object(true)); // object

// `new Object(null)` is also an object (already worked via the `new` path).
const n = new Object(null);
console.log(typeof n); // object
