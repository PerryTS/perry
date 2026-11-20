// Refs v0.5.749: dynamic instanceof — `value instanceof type` where
// `type` is a function arg holding a class ref. Pre-fix the codegen saw
// `ty = "type"` (the param name) and fell through to class_id=0, every
// dynamic-instanceof returned false. Drizzle's `is(value, type)` chain
// depends on this.
class SQL {
    static kind = "SQL";
}
class Aliased {
    static kind = "Aliased";
}
class Other {}

function isType(value: any, type: any): boolean {
    return value instanceof type;
}

const sql = new SQL();
const al = new Aliased();
const o = new Other();

console.log("isType(sql, SQL):", isType(sql, SQL));
console.log("isType(sql, Aliased):", isType(sql, Aliased));
console.log("isType(al, Aliased):", isType(al, Aliased));
console.log("isType(al, SQL):", isType(al, SQL));
console.log("isType(o, SQL):", isType(o, SQL));
console.log("isType(o, Other):", isType(o, Other));

// Also via local-typed binding
const T: any = SQL;
console.log("sql instanceof T:", sql instanceof T);
console.log("al instanceof T:", al instanceof T);
