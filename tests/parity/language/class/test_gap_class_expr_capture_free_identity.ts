// Issue #11298: every evaluation of a class expression produces a distinct
// constructor and a distinct prototype (ClassDefinitionEvaluation), even when
// the class captures nothing and has no statics. Perry lowered the
// capture-free case to one shared template class, so two calls to the same
// factory returned the SAME constructor and a property defined on the second
// evaluation's prototype was visible to instances of the first.

function makeClass() {
  return class {
    constructor() {}
  };
}
const K1 = makeClass();
const K2 = makeClass();
let seen = -1;
Object.defineProperty(K2.prototype, "k", {
  set(v: number) {
    seen = v;
  },
  configurable: true,
});
const o: any = new K1();
o.k = 4;
console.log(
  "issue:",
  seen,
  Object.prototype.hasOwnProperty.call(o, "k"),
  K1 === K2,
  Object.getPrototypeOf(o) === K2.prototype,
);

// Named class expression with methods, accessors and a static method.
function mk() {
  return class Named {
    x = 1;
    static s() {
      return "s";
    }
    m() {
      return this.x + 1;
    }
    get g() {
      return 7;
    }
  };
}
const A = mk();
const B = mk();
const a = new A();
const b = new B();
console.log("identity:", A === B, A.prototype === B.prototype);
console.log("instanceof:", a instanceof A, a instanceof B, b instanceof B);
console.log(
  "proto:",
  Object.getPrototypeOf(a) === A.prototype,
  Object.getPrototypeOf(b) === B.prototype,
);
console.log("members:", a.m(), b.g, A.s(), B.s(), A.name, typeof A);
(A.prototype as any).extra = 5;
console.log("proto write:", (a as any).extra, (b as any).extra);

// A class bound to a local and constructed in the same function.
function local() {
  const C = class {
    v = 3;
    m() {
      return this.v;
    }
  };
  const c = new C();
  return [c.m(), Object.getPrototypeOf(c) === C.prototype, c instanceof C, C.name];
}
console.log("local:", JSON.stringify(local()), JSON.stringify(local()));

// Class expressions evaluated in a loop.
const classes: any[] = [];
for (let i = 0; i < 3; i++) classes.push(class {});
console.log("loop:", classes[0] === classes[1], classes[1] === classes[2]);

// A statically known parent: each evaluation's prototype still chains to the
// parent's prototype.
class Base {
  hi() {
    return "hi";
  }
}
function sub() {
  return class extends Base {};
}
const S1 = sub();
const S2 = sub();
const s1 = new S1();
console.log(
  "static parent:",
  S1 === S2,
  s1.hi(),
  s1 instanceof Base,
  Object.getPrototypeOf(S1.prototype) === Base.prototype,
  Object.getPrototypeOf(s1) === S1.prototype,
);
