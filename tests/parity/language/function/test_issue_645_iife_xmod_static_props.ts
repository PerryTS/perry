// Closes #645 deeper followup: cross-module IIFE-pattern static
// properties on `function` declarations. drizzle's `sql.raw` /
// `sql.identifier` / `sql.fromList` use this exact pattern —
// `((sql2) => { sql2.raw = ...; })(sql)` adds static methods to the
// `sql` function declaration in `drizzle-orm/sql/sql.js`.
//
// Pre-fix each consumer module emitted its own
// `__perry_extern_closure_<src>__<name>` global with LLVM `internal`
// linkage, so the consumer's closure pointer differed from the source's
// singleton. CLOSURE_DYNAMIC_PROPS is pointer-keyed → IIFE writes
// (in source) and consumer reads (in entry.ts) hit different entries.
// Symptom: consumer sees `typeof fn.raw === "undefined"` and
// `fn.raw('a') === "[object Object]"` even though the source's local
// references show `fn.raw('a') === "raw:a"`.
//
// This same-module repro covers the call-site bundling path. The
// actual cross-module export-star case (drizzle's
// `drizzle-orm/sql/sql.js` re-exported through `drizzle-orm/index.js`)
// would need a multi-file fixture; this single-file test verifies the
// underlying pattern works in the simpler case so regressions to it
// are caught at the gap-suite level. The real cross-module case is
// exercised by the drizzle-sqlite acceptance fixture.

function fn(x: any): any {
    return "called " + x;
}

((fn2: any) => {
    fn2.raw = function raw(s: any) {
        return "raw:" + s;
    };
    fn2.id = function id(s: any) {
        return "id:" + s;
    };
})(fn);

console.log("typeof fn=", typeof fn);
console.log("typeof fn.raw=", typeof (fn as any).raw);
console.log("typeof fn.id=", typeof (fn as any).id);
console.log("fn.raw('a')=", (fn as any).raw("a"));
console.log("fn.id('b')=", (fn as any).id("b"));
