// Refs v0.5.756 / #446 followup: `obj.method` on an Any-typed receiver
// now returns a bound-method closure (typeof "function"; calling it
// dispatches to the method via the class vtable chain). Pre-fix the
// runtime path returned undefined for class methods on Any-typed
// receivers because the codegen #446 bound-method-closure arm at
// expr.rs::PropertyGet only fired when the receiver's static class
// was known. Drizzle's `(ins as any)._prepare()` chain reaches into
// methods through Any-typed locals, so this is load-bearing.
class Parent {
    parentMethod(): string {
        return "parent-result";
    }
}
class Child extends Parent {
    childMethod(): string {
        return "child-result";
    }
}

function check(c: any): void {
    // typeof works for both own and inherited methods.
    console.log("typeof child:", typeof c.childMethod);
    console.log("typeof parent:", typeof c.parentMethod);

    // Direct call works.
    console.log("call child:", c.childMethod());
    console.log("call parent:", c.parentMethod());

    // Read-as-value then call (the load-bearing pattern for drizzle's
    // `_prepare()` and similar shapes that hold a method reference).
    const m = c.parentMethod;
    console.log("typeof m:", typeof m);
    console.log("m():", m());
}

check(new Child());
