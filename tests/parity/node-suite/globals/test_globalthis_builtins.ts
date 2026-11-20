// Built-in constructor reads via `globalThis` — unblocks lodash's
// `runInContext` factory which binds `var Array = context.Array; var
// arrayProto = Array.prototype` and similar pre-ES6 patterns. Pre-fix
// these reads returned `undefined`, so the chained `.prototype` access
// threw `TypeError: Cannot read properties of undefined`.
const ArrayRef = globalThis.Array;
const ObjectRef = globalThis.Object;
const FunctionRef = globalThis.Function;
console.log(typeof ArrayRef);
console.log(typeof ObjectRef);
console.log(typeof FunctionRef);
console.log(typeof globalThis.Math);
console.log(typeof globalThis.JSON);

// `.prototype` reads on the locally-bound aliases must return an
// object (the singleton populator stashes an empty object under
// `prototype` for each constructor); pre-fix this read threw because
// the alias was `undefined`. Chained `.toString` etc. still return
// `undefined` rather than the real method — best-effort enough that
// lodash's `var funcToString = funcProto.toString` no longer throws.
const arrayProto = ArrayRef.prototype;
console.log(typeof arrayProto);

// Bare `new Array(n)` still flows through codegen's `lower_new` arm —
// the singleton constructor sentinels don't intercept this path.
const arr = new Array(3);
console.log(arr.length);
