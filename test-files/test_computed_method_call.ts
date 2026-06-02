// Refs v0.5.754: `obj[strKey](args)` computed-key method call now
// dispatches through the class vtable chain (parent inheritance
// included). Pre-fix this fell through to the generic call path that
// read obj[index] as a value (returning undefined for class methods)
// and then tried to call undefined. Drizzle's
// `this.session[isOneTimeQuery ? "prepareOneTimeQuery" : "prepareQuery"](...)`
// chain depends on this.
class Parent {
    parentMethod(): string {
        return "parent";
    }
}
class Child extends Parent {
    childMethod(): string {
        return "child";
    }
}

const c: any = new Child();
const k1 = "childMethod";
const k2 = "parentMethod";

console.log("call child via key:", c[k1]());
console.log("call parent via key:", c[k2]());

// Conditional key — drizzle's actual shape
const useA = true;
class Foo {
    methodA(): string {
        return "A";
    }
    methodB(): string {
        return "B";
    }
}
const f: any = new Foo();
console.log("conditional A:", f[useA ? "methodA" : "methodB"]());
console.log("conditional B:", f[!useA ? "methodA" : "methodB"]());

// With args
class Calc {
    add(x: number, y: number): number {
        return x + y;
    }
}
const calc: any = new Calc();
console.log("args:", calc["add"](2, 3));
