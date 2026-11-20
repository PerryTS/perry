// Refs #645 deeper followup / #488 drizzle-sqlite — HIR default-fill
// must push something for EVERY missing slot (even None defaults), or
// later `Some` defaults land at the wrong positional index.
//
// Drizzle's `tableBase(name, columns, extraConfig?, schema?, baseName=name)`
// is the load-bearing case: a 3-arg call slot 3 (`schema`) had None
// default and slot 4 (`baseName`) had `Some(name)`. Pre-fix the loop
// pushed baseName's default into the schema slot, so the rendered SQL
// became `"users"."users"` instead of `"users"`.

function tableBase<T extends string>(
    name: string,
    columns: any,
    extraConfig?: (self: any) => any,
    schema?: string,
    baseName: string = name,
): any {
    return {
        name,
        schema: schema === undefined ? "undef" : schema,
        baseName,
        hasExtraConfig: typeof extraConfig === "function",
    };
}

const r = tableBase("users", { id: 1 }, undefined);
console.log("name=" + r.name);
console.log("schema=" + r.schema);
console.log("baseName=" + r.baseName);
console.log("hasExtraConfig=" + r.hasExtraConfig);

const r2 = tableBase("orders", { id: 1 }, (self: any) => [self.id]);
console.log("--- with extraConfig ---");
console.log("name=" + r2.name);
console.log("schema=" + r2.schema);
console.log("baseName=" + r2.baseName);
console.log("hasExtraConfig=" + r2.hasExtraConfig);

// Simpler: schema=None, baseName=Some(name) — caller provides only name
function f1(name: string, schema?: string, baseName: string = name) {
    console.log("[f1] name=" + name + " schema=" + (schema ?? "undef") + " baseName=" + baseName);
}
f1("u");

// Mid-call default: caller provides 2 args, default fills the 3rd
function f2(a: string, b?: string, c: string = "C") {
    console.log("[f2] a=" + a + " b=" + (b ?? "undef") + " c=" + c);
}
f2("A", "B");
f2("A");
