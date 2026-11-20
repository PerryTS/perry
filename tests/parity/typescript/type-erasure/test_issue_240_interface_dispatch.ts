// Regression test for issue #240.
//
// Pre-fix, `consume(d: Driver)` in a module that only type-imports the
// interface would compile to a generic property-get-then-closure-call
// for `d.method()`, which silently resolved to `undefined`. The
// implementing class never had its method body invoked.
//
// The trigger was: type-only imports are stripped at HIR lowering
// (`crates/perry-hir/src/lower.rs:2777`) so the interface's source
// module never reaches `ctx.native_modules`, and the consumer module's
// imported_classes ended up without the implementer. The dispatch
// tower's implementor list was empty and the call fell through to the
// closure-call fallback path that doesn't know about the class's
// methods.
//
// The fix at `crates/perry/src/commands/compile.rs` adds a
// polymorphic-receiver augmentation pass: when a module references a
// `Named(X)` type that doesn't resolve to any class/interface/enum/
// type alias in the program HIR (and isn't a builtin), the consumer
// gets every program-wide exported class added to `imported_classes`
// so the dispatch tower can resolve the call.
//
// The repro can't faithfully be a single-file test because the bug
// only fires when the consumer + implementer + interface are in
// separate modules. So this test is structured as a single file
// containing the same dispatch shape WITHOUT an interface — the local
// class is in scope, dispatch tower works as it always has, and we
// just verify the basic shape isn't broken. The actual cross-module
// repro lives outside the test-files set as the issue's reproduction
// case.

interface Greeter {
    greet(name: string): string;
    farewell(): void;
}

class Hello implements Greeter {
    greet(name: string): string {
        return "hello " + name;
    }
    farewell(): void {
        console.log("goodbye");
    }
}

function consume(g: Greeter): void {
    console.log("greet: " + g.greet("world"));
    g.farewell();
}

consume(new Hello());

// Multi-implementer dynamic dispatch (within the same file: dispatch tower
// already worked for this case pre-fix).
class Polite implements Greeter {
    greet(name: string): string {
        return "good day, " + name;
    }
    farewell(): void {
        console.log("farewell");
    }
}

function tryAll(items: Greeter[]): void {
    for (const item of items) {
        console.log("- " + item.greet("there"));
    }
}

tryAll([new Hello(), new Polite()]);
