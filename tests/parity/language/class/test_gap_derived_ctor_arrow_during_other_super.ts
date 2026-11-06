// An arrow written in one derived constructor reads that constructor's
// `this`. Calling it later, while a DIFFERENT derived constructor is still
// inside its super() call, must see the first object (initialized long ago),
// not throw the "must call super" ReferenceError that belongs to the second.
class Base0 {
  r: any;
  constructor(cb?: () => any) {
    if (cb) this.r = cb();
  }
}

class A extends Base0 {
  tag = "A";
  get: () => string;
  early: () => string;
  constructor() {
    const early = () => this.tag + "-early";
    super();
    this.early = early;
    this.get = () => this.tag;
  }
}
const a = new A();

class B extends Base0 {
  constructor() {
    super(a.get);
  }
}
console.log("B sees", new B().r);

class C extends Base0 {
  constructor() {
    super(a.early);
  }
}
console.log("C sees", new C().r);

// Nested: the arrow is called while two derived constructors are mid-super.
class Inner extends Base0 {
  constructor() {
    super(() => a.get() + "+inner");
  }
}
class Outer extends Base0 {
  inner: any;
  constructor() {
    super(() => new Inner().r + "+outer");
  }
}
console.log("Outer sees", new Outer().r);

// The arrow of an object still under construction keeps throwing.
class D extends Base0 {
  constructor() {
    let leak: (() => any) | undefined;
    try {
      const f = () => this;
      leak = f;
      super(f);
    } catch (e: any) {
      console.log("D throws", e instanceof ReferenceError);
      super();
    }
  }
}
new D();

// Derived class whose own super() runs a sibling's arrow that itself
// constructs another derived instance.
class E extends A {
  constructor() {
    super();
    this.tag = "E";
  }
}
const e = new E();
class F extends Base0 {
  constructor() {
    super(() => [e.get(), e.early(), new B().r].join(","));
  }
}
console.log("F sees", new F().r);
