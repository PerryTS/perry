// Refs #618 / #420: static properties added to a class via the IIFE
// pattern `((SQL2) => { SQL2.Aliased = Aliased; })(SQL)` now persist.
// Drizzle-orm's SQL.Aliased shape was the load-bearing site.
class SQL {
    queryChunks: any;
    constructor(c: any) { this.queryChunks = c; }
    static kind = "SQL";
}

((SQL2: any) => {
    class Aliased {
        sql: any;
        constructor(s: any) { this.sql = s; }
        static kind = "SQL.Aliased";
    }
    SQL2.Aliased = Aliased;
})(SQL);

const A = (SQL as any).Aliased;
console.log("A defined:", A !== undefined);

// Multiple static props of various kinds (numbers, strings, objects).
class C {}
((C2: any) => {
    C2.foo = 1;
    C2.bar = "two";
    C2.baz = { x: 3 };
})(C);

console.log("C.foo:", (C as any).foo);
console.log("C.bar:", (C as any).bar);
console.log("C.baz.x:", (C as any).baz.x);
