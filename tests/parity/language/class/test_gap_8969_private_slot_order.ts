// #8969: a private field's entry is claimed where it is declared, so a
// public field defined after it (in the same class or in a subclass) is past
// it in the instance's key order: `[parent, #ps, child]`.

class P {
    parent = 1;
    #ps = 2;
    pt(): number {
        return this.parent + this.#ps;
    }
}
class C extends P {
    child = 3;
}
const c = new C();
console.log("inherited", c.child, c.parent, c.pt());

// Nothing escapes: a candidate for scalar replacement, whose private claims
// still need the object.
class Q {
    parent = 1;
    #ps = 2;
}
class D extends Q {
    child = 3;
    #cs = 4;
}
const d = new D();
console.log("scalar", d.child, d.parent);

class A {
    a = 1;
    #p = 2;
    b = 3;
    get(): number {
        return this.#p;
    }
}
const a = new A();
console.log("base", a.b, a.a, a.get());

// A local read after a method call that updates the private field.
class Counter {
    #n = 0;
    label = "c";
    bump(): number {
        this.#n += 1;
        return this.#n;
    }
}
const k = new Counter();
k.bump();
console.log("counter", k.label, k.bump());
