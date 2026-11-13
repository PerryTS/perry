// Constructing one object does not change what another object is: a method
// call on an earlier object resolves the same way after a later `new`,
// unless that construction really can run other code.
class V {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
  dot(o: V): number {
    return this.x * o.x + this.y * o.y;
  }
  sum(): number {
    return this.x + this.y;
  }
}

// Two plain constructions, then a method on the first.
let s = 0;
for (let i = 0; i < 1000; i++) {
  const a = new V(i, 1);
  const b = new V(2, i);
  s += a.dot(b);
}
console.log("two objects", s);

// The second construction keeps a reference to the first.
class Holder {
  held: V;
  tag: number;
  constructor(held: V, tag: number) {
    this.held = held;
    this.tag = tag;
  }
}
for (let i = 0; i < 3; i++) {
  const a = new V(i, 1);
  const h = new Holder(a, 7);
  h.held.x = h.held.x + 10;
  console.log("held", a.sum(), h.held === a, h.tag);
}

// A constructor that replaces a method of its argument.
class Meddler {
  constructor(target: V) {
    (target as any).sum = () => -1;
  }
}
for (let i = 0; i < 2; i++) {
  const a = new V(i, 1);
  const m = new Meddler(a);
  console.log("meddled", a.sum(), m instanceof Meddler);
}

// A constructor that patches the prototype.
class Patcher {
  constructor() {
    (V.prototype as any).sum = function (this: V) {
      return this.x * 1000;
    };
  }
}
{
  const a = new V(3, 4);
  console.log("before patch", a.sum());
  const p = new Patcher();
  console.log("after patch", a.sum(), p instanceof Patcher);
  delete (V.prototype as any).sum;
  (V.prototype as any).sum = function (this: V) {
    return this.x + this.y;
  };
}

// A constructor whose store runs a setter of its own class.
let setterRuns = 0;
class WithSetter {
  private _v = 0;
  set v(n: number) {
    setterRuns++;
    (V.prototype as any).extra = () => n;
    this._v = n;
  }
  get v(): number {
    return this._v;
  }
  constructor(n: number) {
    this.v = n;
  }
}
{
  const a = new V(1, 1);
  const w = new WithSetter(5);
  console.log("setter", setterRuns, (a as any).extra(), w.v);
  delete (V.prototype as any).extra;
}

// A constructor that computes with its argument runs the argument's valueOf.
class Scaled {
  k: number;
  constructor(k: number) {
    this.k = k * 2;
  }
}
{
  const a = new V(1, 2);
  const sneaky = {
    valueOf() {
      (a as any).sum = () => 99;
      return 4;
    },
  };
  const sc = new Scaled(sneaky as any);
  console.log("valueOf", sc.k, a.sum());
}

// A subclass constructor runs its parent's.
class Base {
  constructor(target: V) {
    (target as any).dot = () => 123;
  }
}
class Derived extends Base {
  z: number;
  constructor(target: V) {
    super(target);
    this.z = 1;
  }
}
{
  const a = new V(1, 2);
  const b = new V(3, 4);
  const d = new Derived(a);
  console.log("parent", a.dot(b), d.z);
}

// A default parameter value is code too.
class WithDefault {
  n: number;
  constructor(target: V, n: number = ((target as any).sum = () => 7, 1)) {
    this.n = n;
  }
}
{
  const a = new V(1, 2);
  const w = new WithDefault(a);
  console.log("default", a.sum(), w.n);
}

// A field initializer is code too.
const shared = new V(5, 5);
class WithInit {
  n = ((shared as any).sum = () => 55, 2);
}
{
  const before = shared.sum();
  const w = new WithInit();
  console.log("initializer", before, shared.sum(), w.n);
}
