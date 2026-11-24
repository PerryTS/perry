// `super.m()` in a static method reads the home class's [[Prototype]] when
// the method is called through the class's own function object: a class
// declared in a function and evaluated more than once, and a static
// generator, whose bodies find the home through the method value.

function make() {
  class A {
    static m() {
      return "A.m";
    }
  }
  class B extends A {
    static m() {
      return "B>" + super.m();
    }
    static k() {
      return "B.k>" + super.m();
    }
  }
  return B;
}
console.log(make().m());
console.log(make().m());
console.log(make().k());

function family() {
  class P {
    static m() {
      return "P.m";
    }
  }
  class C extends P {
    static m() {
      return "C>" + super.m();
    }
  }
  return [P, C] as const;
}
const [P1, C1] = family();
console.log(C1.m());
const [P2, C2] = family();
(P2 as any).m = () => "P2 replaced";
console.log(C2.m());
console.log(C1.m());

class GA {
  static *g() {
    yield "GA.g";
  }
}
class GB extends GA {
  static *g() {
    yield "GB";
    yield* super.g();
  }
}
console.log([...GB.g()].join(","));
const g = GB.g;
console.log([...g.call(GB)].join(","));
