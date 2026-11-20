// JSON.stringify of class instances: the toJSON question answered from the
// prototype chain, and the per-key enumeration of class layouts, must match
// Node in every case where a holder, the instance or the replacer changes
// what is visible.

class P {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}

// Plain class instances, many times (the walk must stay right after warm-up).
let acc = "";
for (let i = 0; i < 3; i++) acc += JSON.stringify(new P(i, i + 1));
console.log("plain", acc);
console.log("array", JSON.stringify([new P(1, 2), new P(3, 4)]));
console.log("nested", JSON.stringify({ a: new P(1, 2), b: [new P(5, 6)] }));

// toJSON on the class prototype.
class WithToJson {
  v = 1;
  toJSON() {
    return { tagged: this.v };
  }
}
console.log("proto toJSON", JSON.stringify(new WithToJson()));

// toJSON on a parent prototype.
class Base {
  toJSON() {
    return "base";
  }
}
class Derived extends Base {
  d = 2;
}
console.log("parent toJSON", JSON.stringify(new Derived()));

// toJSON added to a prototype after the first call.
class Later {
  a = 1;
}
const later = new Later();
console.log("before add", JSON.stringify(later));
(Later.prototype as any).toJSON = function () {
  return "added later";
};
console.log("after add", JSON.stringify(later));
console.log("after add, new instance", JSON.stringify(new Later()));
delete (Later.prototype as any).toJSON;
console.log("after delete", JSON.stringify(later));

// toJSON as an accessor on the prototype.
class AccessorHolder {
  q = 3;
}
let getterRuns = 0;
Object.defineProperty(AccessorHolder.prototype, "toJSON", {
  get() {
    getterRuns++;
    return () => "from accessor";
  },
  configurable: true,
});
console.log("accessor toJSON", JSON.stringify(new AccessorHolder()), getterRuns);

// Object.prototype: its shape is the last holder the chain walk reads.
// (A compiled program cannot add Object.prototype.toJSON, DEC 250.)
class Q {
  z = 9;
}
console.log("objproto holder", JSON.stringify(new Q()), "toJSON" in new Q());

// setPrototypeOf on an instance.
const re = new P(7, 8);
Object.setPrototypeOf(re, { toJSON: () => "relinked" });
console.log("setPrototypeOf", JSON.stringify(re), JSON.stringify(new P(7, 8)));
const re2 = new P(1, 1);
Object.setPrototypeOf(re2, null);
console.log("setPrototypeOf null", JSON.stringify(re2));

// Own toJSON on one instance only.
const own = new P(1, 2) as any;
own.toJSON = () => "own";
console.log("own toJSON", JSON.stringify(own), JSON.stringify(new P(1, 2)));

// A toJSON result that is a class instance whose field has a toJSON: the
// field's toJSON still runs (only the result itself is not re-asked).
class Inner {
  toJSON() {
    return "inner-tojson";
  }
}
class Box {
  inner = new Inner();
  n = 1;
}
class Outer {
  toJSON() {
    return new Box();
  }
}
console.log("result field toJSON", JSON.stringify(new Outer()));
console.log("result field toJSON in array", JSON.stringify([new Outer(), new Box()]));
// The same with a plain-record result.
class OuterPlain {
  toJSON() {
    return { inner: new Inner(), n: 2 };
  }
}
console.log("plain result field toJSON", JSON.stringify(new OuterPlain()));

// A getter that adds a key while serializing.
class Grows {
  a = 1;
  b = 2;
}
const grows = new Grows() as any;
Object.defineProperty(grows, "g", {
  enumerable: true,
  get() {
    this.late = "added";
    return "getter";
  },
});
console.log("getter adds key", JSON.stringify(grows), JSON.stringify(grows));

// Replacer function and replacer array.
console.log(
  "replacer fn",
  JSON.stringify(new P(1, 2), (k, v) => (k === "x" ? v * 10 : v)),
);
console.log("replacer array", JSON.stringify(new P(1, 2), ["y"]));

// Symbol key.
const sym = new P(1, 2) as any;
sym[Symbol("s")] = "hidden";
console.log("symbol key", JSON.stringify(sym));

// Deleted key.
const del = new P(1, 2) as any;
delete del.x;
console.log("deleted key", JSON.stringify(del));

// Class accessor (on the prototype: not an own property, not serialized).
class WithAccessor {
  _v = 4;
  get v() {
    return this._v * 2;
  }
}
console.log("class accessor", JSON.stringify(new WithAccessor()));

// Private field.
class WithPrivate {
  #secret = 1;
  shown = 2;
  peek() {
    return this.#secret;
  }
}
console.log("private field", JSON.stringify(new WithPrivate()));

// A field declared but never assigned in the constructor.
class Sparse {
  a: number;
  b?: number;
  c: number;
  constructor(flag: boolean) {
    this.a = 1;
    if (flag) this.b = 2;
    this.c = 3;
  }
}
console.log("never assigned", JSON.stringify(new Sparse(false)), JSON.stringify(new Sparse(true)));

// A non-enumerable key defined on an instance.
const ne = new P(1, 2);
Object.defineProperty(ne, "x", { enumerable: false });
Object.defineProperty(ne, "hiddenExtra", { value: 5, enumerable: false });
console.log("non-enumerable", JSON.stringify(ne), Object.keys(ne).join(","));

// An undefined-valued field and a function-valued field.
class Odd {
  u: any = undefined;
  f = () => 1;
  k = "kept";
}
console.log("undefined and fn fields", JSON.stringify(new Odd()));

// Slice B (per-key attributes from the shape summary): the summary is read
// per key, so changes made by user code during the walk stay visible.

// A getter that deletes a later key during the walk.
class Del2 {
  a = 1;
  b = 2;
  c = 3;
}
const del2 = new Del2() as any;
Object.defineProperty(del2, "a", {
  enumerable: true,
  configurable: true,
  get() {
    delete this.b;
    return "a-getter";
  },
});
console.log("getter deletes later key", JSON.stringify(del2), JSON.stringify(del2));

// A member toJSON that deletes a later key of its holder.
class Holder {
  m: any;
  n = "n";
  o = "o";
  constructor() {
    const self = this;
    this.m = {
      toJSON() {
        delete (self as any).n;
        return "m";
      },
    };
  }
}
console.log("member toJSON deletes key", JSON.stringify(new Holder()));

// A member toJSON that installs an accessor on its holder (the holder had none).
class Grows2 {
  m: any;
  late = 1;
  constructor() {
    const self = this;
    this.m = {
      toJSON() {
        Object.defineProperty(self, "late", { get: () => "late-getter", enumerable: true, configurable: true });
        return "m";
      },
    };
  }
}
console.log("member toJSON adds accessor", JSON.stringify(new Grows2()));

// A subclass re-declaring a field (both names live in the class layout).
class Base2 {
  tag = 1;
  a = "a";
}
class Re extends Base2 {
  tag = 2;
}
console.log("redeclared field", JSON.stringify(new Re()));
const reNe = new Re();
Object.defineProperty(reNe, "tag", { enumerable: false });
console.log("redeclared non-enumerable", JSON.stringify(reNe), Object.keys(reNe).join(","));
const reAcc = new Re();
Object.defineProperty(reAcc, "tag", { get: () => 9, enumerable: true, configurable: true });
console.log("redeclared accessor", JSON.stringify(reAcc));

// Private field plus an own accessor on the instance.
class PrivAcc {
  #p = 1;
  shown = 2;
  peek() {
    return this.#p;
  }
}
const pa = new PrivAcc();
Object.defineProperty(pa, "shown", { get: () => "via-getter", enumerable: true, configurable: true });
console.log("private + own accessor", JSON.stringify(pa), JSON.stringify(new PrivAcc()));

// An optional field never assigned, then a non-enumerable sibling.
class Opt {
  a?: number;
  b = 1;
  c = 2;
}
const opt = new Opt();
Object.defineProperty(opt, "c", { enumerable: false });
console.log("optional + non-enumerable", JSON.stringify(opt), JSON.stringify(new Opt()));
