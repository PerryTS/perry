// Refs #488 drizzle-sqlite: `arr.push(...src)` where the receiver is
// a property of an object (e.g. drizzle's `result.params.push(...query.params)`
// in mergeQueries) was a silent no-op. Pre-fix the HIR lowered to
// `NativeMethodCall { module: "array", method: "push_spread" }` but
// codegen had no arm for `push_spread`, so it fell through to the
// catch-all that lowered receiver+args for side effects and returned
// `0.0`. The push never happened — drizzle's INSERT went out with 0
// params and silently inserted nothing.

// Direct array var: already worked pre-fix
const a: number[] = [];
a.push(...[1, 2, 3]);
console.log("direct:", JSON.stringify(a));

// Object field (drizzle's shape): pre-fix this was the broken case
const b: any = { params: [] };
b.params.push(...[10, 20, 30]);
console.log("field:", JSON.stringify(b.params));

// Iterated accumulation (drizzle's mergeQueries shape)
const queries = [
    { sql: "a", params: [1] },
    { sql: "b", params: ["alice"] },
    { sql: "c", params: [3.14, true, null] },
];
const result: any = { sql: "", params: [] };
for (const q of queries) {
    result.sql += q.sql;
    result.params.push(...q.params);
}
console.log("merged sql:", result.sql);
console.log("merged params:", JSON.stringify(result.params));
