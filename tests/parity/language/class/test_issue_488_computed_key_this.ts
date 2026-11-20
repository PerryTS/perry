// Refs #488 drizzle-sqlite: computed-key object literal with `this.X`
// must capture `this` lexically through the IIFE wrapper. Pre-fix
// drizzle's `{ [this.tableName]: true }` in SQLiteSelectQueryBuilderBase
// ctor threw `Cannot read properties of undefined (reading 'tableName')`
// because the IIFE's `captures_this` was hardcoded to `false`.
class Foo {
    name: string = "hello";
    test(): any {
        return { [this.name]: true };
    }
    inTernary(): any {
        return typeof this.name === "string" ? { [this.name]: true } : {};
    }
}
const f = new Foo();
console.log("test:", JSON.stringify(f.test()));
console.log("ternary:", JSON.stringify(f.inTernary()));
