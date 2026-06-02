// Closes #645 (deeper followup): rest-param functions re-exported via
// `export * from "./other"` lost their has_rest bit through the
// `exported_func_has_rest` propagation. Consumers that imported the
// re-exported name through the barrel module saw `params[0] === undefined`
// because the call site didn't bundle the user-supplied args into the
// rest array. Symptom on drizzle-sqlite: drizzle's `drizzle(sqlite)`
// re-exported through `drizzle-orm/better-sqlite3/index.js` hit
// `params[0] === undefined` and took the `new Client()` (no-arg) branch
// instead of using the user's Database instance.
//
// The actual cross-module export-star case lives in
// `tests/release/packages/drizzle-sqlite/`. This single-file program
// verifies the call-site rest-bundling path works correctly when the
// function lives in the same module — same codegen path, same regression
// shape.

function rest(...params: any[]): number {
    return params.length;
}

console.log("rest()=", rest());
console.log("rest(1)=", rest(1));
console.log("rest(1, 2, 3)=", rest(1, 2, 3));
