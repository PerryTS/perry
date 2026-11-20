// `super.m()` in a static method reads `m` on the home class's current
// [[Prototype]] with `this` as the receiver: a replaced parent static, a
// deleted one, a relinked parent and a `.call` receiver all stay observable
// after the site has run hot (#12213).

class Base {
  static tag = "Base";
  static m(x: number, y?: number) {
    return `${this.tag}:Base.m(${x},${y})`;
  }
  static deep(x: number) {
    return `${this.tag}:Base.deep(${x})`;
  }
}
class Mid extends Base {
  static tag = "Mid";
}
class Sub extends Base {
  static tag = "Sub";
  static m(x: number, y?: number) {
    return "Sub>" + super.m(x, y);
  }
  static few() {
    return super.m(1);
  }
  static extra() {
    return super.m(1, 2, (globalThis as any).sideEffect());
  }
}
class Leaf extends Mid {
  static tag = "Leaf";
  static deep(x: number) {
    return "Leaf>" + super.deep(x);
  }
}

let effects = 0;
(globalThis as any).sideEffect = () => ++effects;

function hot(label: string, f: () => string) {
  let last = "";
  for (let i = 0; i < 2000; i++) last = f();
  console.log(label, last);
}

hot("plain", () => Sub.m(3, 4));
hot("few", () => Sub.few());
hot("extra", () => Sub.extra());
console.log("effects", effects);
hot("deep", () => Leaf.deep(5));
console.log("call", Sub.m.call({ tag: "obj" }, 6, 7));
class Sub2 extends Sub {
  static tag = "Sub2";
}
console.log("inherited receiver", Sub2.m(8, 9));

// The get precedes the arguments: an argument that replaces Base.m does not
// change the callee of this call, only of the next.
const original = Base.m;
class Order extends Base {
  static tag = "Order";
  static go() {
    return super.m(
      (() => {
        Base.m = function (this: any, x: number) {
          return `${this.tag}:replaced(${x})`;
        } as any;
        return 10;
      })(),
    );
  }
}
console.log("order 1", Order.go());
console.log("order 2", Order.go());
Base.m = original;
console.log("restored", Order.go());

// Replace the parent's static after the site ran hot.
Base.m = function (this: any, x: number) {
  return `${this.tag}:swapped(${x})`;
} as any;
hot("replaced", () => Sub.m(11));
Base.m = original;
hot("restored", () => Sub.m(12));

// Delete it: the read falls through to Function.prototype (undefined) and
// the call throws.
delete (Base as any).m;
try {
  Sub.m(13);
} catch (e: any) {
  console.log("deleted", e instanceof TypeError);
}
(Base as any).m = original;
hot("re-added", () => Sub.m(14));

// A static the intermediate class gains shadows the grandparent's.
(Mid as any).deep = function (this: any, x: number) {
  return `${this.tag}:Mid.deep(${x})`;
};
hot("mid shadow", () => Leaf.deep(15));
delete (Mid as any).deep;
hot("mid unshadowed", () => Leaf.deep(16));

// Relink the home's [[Prototype]].
const Other = {
  tag: "Other",
  m(this: any, x: number) {
    return `${this.tag}:Other.m(${x})`;
  },
};
Object.setPrototypeOf(Sub, Other);
hot("relinked", () => Sub.m(17));
Object.setPrototypeOf(Sub, Base);
hot("linked back", () => Sub.m(18));

// A getter on the new parent runs with the original receiver.
Object.setPrototypeOf(Sub, {
  get m() {
    const self = this;
    return (x: number) => `getter saw ${(self as any).tag} (${x})`;
  },
});
console.log("getter", Sub.m(19));
Object.setPrototypeOf(Sub, Base);
console.log("final", Sub.m(20));
