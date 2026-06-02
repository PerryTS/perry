// Regression: ramda's curry/variadic helpers (`_curry1`, `_curry2`, the
// `keys.js`/`_isArguments.js` IIFEs, `converge`/`juxt`/`useWith` chain)
// throw `Cannot read properties of undefined (reading 'call')` /
// `value is not a function` / `First argument to _arity must be a
// non-negative integer no greater than ten` at module init unless Perry
// exposes:
//
//   - `Array.prototype.slice` as a callable closure that dispatches to
//     `js_array_slice` via `IMPLICIT_THIS`,
//   - `Object.prototype.toString` as a `[object Tag]` thunk,
//   - `Object.prototype.hasOwnProperty` / `propertyIsEnumerable` as
//     duck-typed runtime arms,
//   - `Function.prototype.length` (i.e. `fn.length`) returning the
//     spec-correct declared-param count for top-level user-function
//     wrappers via the closure-arity registry.
//
// This test covers the synthetic mini-reproducers that pin the runtime
// + codegen surface listed above. End-to-end `R.sum([1,2,3,4,5])` still
// blocks on the transducer prototype-on-callable pattern
// (`XWrap.prototype['@@transducer/step']`) which is tracked as a
// follow-up.

// Array.prototype.slice.call shape used by ramda's _curry1 etc.
const sliced = Array.prototype.slice.call([10, 20, 30, 40], 1);
console.log(sliced);

const sliced_end = Array.prototype.slice.call([10, 20, 30, 40], 1, 3);
console.log(sliced_end);

// Object.prototype.toString.call shape used by ramda's _isString /
// _isObject / _isRegExp / _isArguments IIFEs.
console.log(Object.prototype.toString.call([1, 2, 3]));
console.log(Object.prototype.toString.call(null));
console.log(Object.prototype.toString.call(undefined));

// Object.prototype.hasOwnProperty / propertyIsEnumerable used by
// _has.js / _clone.js / keys.js (the IIFE that branches on
// `!{toString:null}.propertyIsEnumerable('toString')`).
const ohp: any = { a: 1 };
console.log(ohp.hasOwnProperty("a"));
console.log(({ toString: null } as any).propertyIsEnumerable("toString"));

// Function.prototype.length — ramda's `converge` / `juxt` chain
// reads `pluck('length', fns)` to compute the curry arity, then
// feeds it through `reduce(max, 0, ...)` → `_arity(N, ...)`. NaN
// here trips ramda's `_arity` bound check at module init.
function three(a: any, b: any, c: any) { return a + b + c; }
function zero() { return 0; }
console.log(three.length);
console.log(zero.length);

// Mini-repro for the `function f(){} const g = f; g.call(null);` shape
// (PR description noted this should be exercised explicitly).
function greet(this: any, name: string) { return "hi " + name; }
const greet_alias = greet;
console.log(greet_alias.call(null, "ramda"));
