// #6588: a compiled dot-read of a `.errors` expando that stores `null`
// returned a hole/pointer sentinel instead of `null`. The codegen fast-path
// unconditionally routed EVERY static `X.errors` read through the Error-only
// `js_error_get_errors` helper (whose `ArrayHeader*` return can't represent a
// stored null). Now gated to statically-Error receivers; everything else uses
// the generic property read, which preserves null. Real AggregateError.errors
// must keep working.

// --- function expando storing null (the reported case) ---
function f() {}
f.errors = null;
console.log(f.errors === null); // true
console.log(typeof f.errors); // object
console.log(String(f.errors)); // null
console.log(f["err" + "ors"] === null); // computed lane — true

// --- plain object with a null `errors` expando (same bug class) ---
const o: any = {};
o.errors = null;
console.log(o.errors === null); // true
console.log(String(o.errors)); // null

// --- non-null function expando (control) ---
function g() {}
g.errors = 42;
console.log(g.errors === 42); // true
console.log(g.errors); // 42

// --- null on a differently-named field (control) ---
const p: any = {};
p.e = null;
console.log(p.e === null); // true

// --- real AggregateError.errors must keep working (no regression) ---
const agg = new AggregateError([new Error("a"), new Error("b")], "multi");
console.log(agg.errors.length); // 2
console.log(agg.errors[0].message); // a
console.log(JSON.stringify(new AggregateError([1, 2], "m").errors)); // [1,2]
