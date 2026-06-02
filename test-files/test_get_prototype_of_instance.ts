// Refs v0.5.752: Object.getPrototypeOf for class instances returns
// the value itself rather than null. Drizzle's `is(value, type)` chain
// reads `Object.getPrototypeOf(value).constructor` — a null return
// throws on `null.constructor`. Returning the value itself routes
// `.constructor` through:
//   - Class instances → class ref (v0.5.746 intercept)
//   - class refs → class ref itself (v0.5.752 intercept) so the
//     chain `getPrototypeOf(instance).constructor === instance.constructor`
//     collapses correctly.
class Foo {
    static kind = "Foo";
}

function go() {
    const f = new Foo();
    const p = Object.getPrototypeOf(f);
    console.log("typeof p:", typeof p);
    // Drizzle's load-bearing chain: getPrototypeOf(instance).constructor
    // resolves to the same class ref as instance.constructor.
    console.log("p.constructor === Foo:", p.constructor === Foo);
    console.log("f.constructor === p.constructor:", f.constructor === p.constructor);
}
go();
