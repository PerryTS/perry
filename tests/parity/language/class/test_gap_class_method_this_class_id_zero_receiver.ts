// A compiled class method called on an object with class id 0 whose
// prototype chain reaches the class prototype must receive that object as
// `this`. `Object.setPrototypeOf(Object.create(null), C.prototype)` gives such
// an object today; once `Object.create` stops minting a synthetic class id per
// call (#11166), every `Object.create(C.prototype)` (the tslib `__extends` /
// Babel `_inherits` shape) and `Object.create(new C())` is one too. The method
// value's captured owner marker leaked as `this` instead, so the body saw
// `typeof this === "number"` and every `this.x` read undefined (#11201).
// Two neighbours of the same receiver shape are pinned too: an inherited
// class SETTER was skipped (the write created an own data property), and a
// class GETTER reading an inherited property of its own receiver saw
// `undefined`.

class Base {
  x = 5;
  label = "base";
  m(): string {
    return "m:" + typeof this + ":" + (this as any).x;
  }
  get g(): string {
    return "g:" + typeof this + ":" + (this as any).x;
  }
  set s(v: number) {
    (this as any).x = v * 10;
  }
  describe(prefix: string, ...rest: number[]): string {
    return prefix + this.label + ":" + this.x + ":" + rest.join(",");
  }
  self(): unknown {
    return this;
  }
}

class Derived extends Base {
  y = 2;
  sum(): number {
    return (this as any).x + this.y;
  }
  m(): string {
    return "derived(" + super.m() + ")";
  }
}

function probe(name: string, o: any, withSetter = true): void {
  console.log(name, "m", o.m());
  console.log(name, "g", o.g);
  o.x = 7;
  console.log(name, "m after write", o.m());
  console.log(name, "g after write", o.g);
  if (withSetter) {
    o.s = 3;
    console.log(name, "setter wrote", o.x, o.hasOwnProperty("x"));
  }
  o.label = "own";
  console.log(name, "describe", o.describe("p:", 1, 2));
  console.log(name, "self is receiver", o.self() === o);
  console.log(name, "call", Base.prototype.m.call(o));
  console.log(name, "apply", Base.prototype.describe.apply(o, ["a:", 9]));
  console.log(name, "bind", Base.prototype.m.bind(o)());
  console.log(name, "instanceof", o instanceof Base);
}

// Null-prototype object re-parented onto the class prototype.
const n = Object.create(null);
Object.setPrototypeOf(n, Base.prototype);
probe("setProto(null-proto)", n);

// Plain object literal re-parented onto the class prototype. A literal carries
// an anonymous shape class id, not 0, so it is the control for the method
// path. (Its inherited class SETTER is still skipped on main; that store takes
// a different route and is not pinned here.)
const lit: any = { tag: 1 };
Object.setPrototypeOf(lit, Base.prototype);
probe("setProto(literal)", lit, false);

// Object.create shapes.
probe("create(proto)", Object.create(Base.prototype));
probe("create(instance)", Object.create(new Base()));

// Subclass methods, super calls and fields read through `this`.
const d = Object.create(null);
Object.setPrototypeOf(d, Derived.prototype);
d.x = 1;
d.y = 4;
console.log("derived m", d.m());
console.log("derived sum", d.sum());
console.log("derived call", Derived.prototype.sum.call(d));
console.log("derived base call", Base.prototype.m.call(d));
const di = Object.create(new Derived());
console.log("derived create(instance)", di.m(), di.sum(), di.g);

// tslib `__extends` shape: a function constructor inheriting from a class.
function Legacy(this: any) {
  this.x = 11;
}
Legacy.prototype = Object.create(Base.prototype);
Legacy.prototype.constructor = Legacy;
const leg = new (Legacy as any)();
console.log("legacy", leg.m(), leg.g, leg.self() === leg);

// Re-parented onto a class INSTANCE: the getter reads an inherited field.
const onInstance = Object.create(null);
Object.setPrototypeOf(onInstance, new Base());
console.log("setProto(instance)", onInstance.g, onInstance.m(), onInstance.describe(">"));

// A method value called on an unrelated class-id-0 object still receives that
// object: `this` is the receiver, whatever its prototype.
const stranger = Object.create(null);
stranger.x = 42;
stranger.label = "s";
console.log("stranger call", Base.prototype.m.call(stranger));
console.log("stranger apply", Base.prototype.describe.apply(stranger, ["q:", 1]));
