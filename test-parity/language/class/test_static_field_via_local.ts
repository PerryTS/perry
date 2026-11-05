// Refs v0.5.747: static field access via Any-typed local holding a class
// ref now works. Pre-fix `const X: any = A; X.kind` returned undefined
// because the codegen's PIC fast path treated INT32-tagged class refs
// (top16=0x7FFE) as non-pointer values and routed to the invalid arm.
// Drizzle's `is(value, type)` chain reads `cls.kind` through Any-typed
// locals — was the load-bearing motivating case.
class A {
    static kind = "A";
    static count = 42;
}

// Direct read still works.
console.log("A.kind:", A.kind);
console.log("A.count:", A.count);

// Read via Any-typed local — was undefined pre-fix.
const X: any = A;
console.log("X.kind:", X.kind);
console.log("X.count:", X.count);
console.log("typeof X:", typeof X);
console.log("X === A:", X === A);
console.log("X.constructor === A:", X === A);

// As-cast (still works; the cast is erased and the lower-level expression
// remains a ClassRef which goes through the direct fast path).
console.log("(A as any).kind:", (A as any).kind);

// Read inside a function — the function arg is Any-typed.
function readKind(c: any): string {
    return c.kind;
}
console.log("readKind(A):", readKind(A));
