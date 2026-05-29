// #2143 — built-in function `.bind`/`.call`/`.apply` on namespace statics.
//
// Pre-fix, reading `.bind` (or `.call` / `.apply`) off a value like
// `Promise.resolve` returned `number` rather than `function`, and calling
// `Promise.resolve.bind(Promise)(x)` threw "value is not a function".
//
// Built-in function values don't inherit `Function.prototype` in Perry's
// representation — each direct call site is special-cased in codegen, with
// no reified function value to hang `.bind`/`.call`/`.apply` off. This test
// pins the typeof folds + the immediate-call shape rewrites that landed
// alongside the issue.

// typeof of the namespace static itself — `function` now.
console.log("typeof Promise.resolve:", typeof Promise.resolve);
console.log("typeof Math.min:", typeof Math.min);
console.log("typeof JSON.parse:", typeof JSON.parse);
console.log("typeof Number.isFinite:", typeof Number.isFinite);
console.log("typeof String.fromCharCode:", typeof String.fromCharCode);

// typeof of the chained `.bind` / `.call` / `.apply` read — `function` now.
console.log("typeof Promise.resolve.bind:", typeof Promise.resolve.bind);
console.log("typeof Math.min.call:", typeof Math.min.call);
console.log("typeof JSON.parse.apply:", typeof JSON.parse.apply);

// `.call(thisArg, …)` — thisArg ignored, args forwarded.
console.log("Math.min.call(null, 3, 1, 2):", Math.min.call(null, 3, 1, 2));
console.log("Math.max.call(null, 3, 1, 2):", Math.max.call(null, 3, 1, 2));

// `.apply(thisArg, [args])` — clean literal args expanded.
console.log("Math.min.apply(null, [3, 1, 2]):", Math.min.apply(null, [3, 1, 2]));

// `.bind(thisArg, …pre)(…rest)` — immediate-call form.
console.log("Math.min.bind(null, 3, 1)(2):", Math.min.bind(null, 3, 1)(2));

// Promise.resolve.bind(Promise)(x) — the issue's headline repro.
Promise.resolve.bind(Promise)(42).then((v: any) => {
    console.log("Promise.resolve.bind(Promise)(42) →", v);
});
