// Two (and more) classes with the same name in different scopes are distinct
// classes: each has its own static data properties, static methods (own
// function objects running their own bodies), inheritance and identity.

function makeA() {
  class Box {
    static tag = "a";
    static count = 1;
    static make(x: number) {
      return x + 1;
    }
    static who() {
      return "A:" + this.tag;
    }
    get kind() {
      return "boxA";
    }
  }
  return Box;
}

function makeB() {
  class Box {
    static tag = "b";
    static make(x: number) {
      return x * 10;
    }
    static who() {
      return "B:" + this.tag;
    }
    static only() {
      return "only-b";
    }
    get kind() {
      return "boxB";
    }
  }
  return Box;
}

const A = makeA();
const B = makeB();
console.log(A.name, B.name, A === B);
console.log(A.tag, B.tag, (A as any).count, (B as any).count);
console.log(A.make(1), B.make(1));
console.log(A.who(), B.who());
console.log(typeof (A as any).only, typeof (B as any).only);
console.log(A.make === B.make, A.who === B.who);
console.log(Object.getOwnPropertyNames(A).join(","));
console.log(Object.getOwnPropertyNames(B).join(","));
console.log(new A().kind, new B().kind);
console.log(new A() instanceof A, new A() instanceof B, new B() instanceof B);

// A write on one never reaches the other.
(A as any).make = (x: number) => x - 100;
console.log(A.make(1), B.make(1));
delete (B as any).who;
console.log(typeof A.who, typeof (B as any).who);

// Block scopes, including one shadowing a module-level class of the same name.
class Pt {
  static origin() {
    return "outer";
  }
  static z = 0;
}
{
  class Pt {
    static origin() {
      return "block1";
    }
    static z = 1;
  }
  console.log(Pt.origin(), Pt.z);
  class Sub extends Pt {}
  console.log(Sub.origin(), Sub.z);
}
{
  class Pt {
    static origin() {
      return "block2:" + this.z;
    }
    static z = 2;
  }
  console.log(Pt.origin(), Pt.z);
}
console.log(Pt.origin(), Pt.z);

// Static calls in loops through each same-named class (the static-call guard).
let s = 0;
for (let i = 0; i < 5; i++) s = A.make(s) + B.make(1);
console.log(s);
