// Reading a class method as a value on a class-typed receiver ([[Get]]): an
// own property of the same name wins — a data value as stored (undefined
// included), an accessor through its getter — and only a missing own property
// falls back to the prototype method. Private names, symbol-keyed methods,
// numeric-like and lone-surrogate names stay in their own namespaces.

class Base {
  m(): string {
    return "Base.m";
  }
  other(): string {
    return "Base.other";
  }
  // `this.m` read as a value inside a method.
  ownM(): any {
    return this.m;
  }
  ownOther(): any {
    return this.other;
  }
}

// `x.m` read as a value on a class-typed parameter.
function readM(x: Base): any {
  return x.m;
}
function readOther(x: Base): any {
  return x.other;
}

class Derived extends Base {
  m(): string {
    return "Derived.m";
  }
}

// Own accessor shadowing a method.
const a = new Base();
Object.defineProperty(a, "m", {
  get() {
    return () => "own getter";
  },
  configurable: true,
});
console.log("accessor:", typeof a.m, a.m());
const viaValue = a.m;
console.log("accessor value:", viaValue());
console.log("accessor via this:", typeof a.ownM(), a.ownM()());
console.log("accessor via param:", typeof readM(a), readM(a)());

// Own accessor without a getter reads undefined.
const a2 = new Base();
Object.defineProperty(a2, "other", { set(_v: unknown) {}, configurable: true });
console.log("setter-only:", typeof a2.other, typeof a2.ownOther(), typeof readOther(a2));

// Own data property holding undefined shadows the method.
const b = new Base();
(b as any).m = undefined;
console.log("own undefined:", typeof b.m, b.m === undefined, "m" in b);
console.log("own undefined via this:", typeof b.ownM(), "via param:", typeof readM(b));
delete (b as any).m;
console.log("after delete:", typeof b.m, b.m(), typeof b.ownM(), typeof readM(b));

// Own data property holding null / a number.
const b2 = new Base();
(b2 as any).m = null;
console.log("own null:", b2.m === null, b2.ownM() === null, readM(b2) === null);
(b2 as any).m = 7;
console.log("own number:", b2.m, b2.ownM(), readM(b2));

// Derived.prototype.m shadows Base.prototype.m.
const d = new Derived();
console.log("derived:", d.m(), d.m === Derived.prototype.m, d.m === Base.prototype.m, d.other());
const asBase: Base = d;
console.log("derived as base:", asBase.m(), readM(d) === Derived.prototype.m, d.ownM() === Derived.prototype.m);

// The bind-in-constructor idiom on an inherited method.
class Binder extends Base {
  constructor() {
    super();
    this.m = this.m.bind(this);
  }
}
const bd = new Binder();
console.log("binder:", bd.m(), Object.prototype.hasOwnProperty.call(bd, "m"), bd.m === Base.prototype.m, readM(bd) === bd.m);

// A private method next to a public one of the same spelling.
class Priv {
  #m(): string {
    return "private";
  }
  m(): string {
    return "public+" + this.#m();
  }
  callPrivate(): string {
    return this.#m();
  }
  privateValue(): any {
    return this.#m;
  }
  publicValue(): any {
    return this.m;
  }
}
const p = new Priv();
console.log("private:", p.m(), p.callPrivate(), Object.keys(p).length);
(p as any).m = () => "own m";
console.log("private after own m:", p.m(), p.callPrivate(), p.privateValue() === p.publicValue(), typeof p.privateValue());
const p2 = new Priv();
Object.defineProperty(p2, "m", { value: undefined, configurable: true });
console.log("private beside own undefined m:", typeof p2.publicValue(), typeof p2.privateValue(), p2.callPrivate());

// Symbol.iterator method against look-alike string keys.
class Iter {
  *[Symbol.iterator](): Generator<number> {
    yield 1;
    yield 2;
  }
}
const it = new Iter();
(it as any)["Symbol(Symbol.iterator)"] = "string key";
(it as any)["Symbol.iterator"] = "another string key";
(it as any)["@@iterator"] = "at-at key";
console.log("symbol:", [...it].join(","), typeof it[Symbol.iterator], (it as any)["Symbol(Symbol.iterator)"]);
console.log("symbol method value:", it[Symbol.iterator] === Iter.prototype[Symbol.iterator]);

// Numeric-like method name against an indexed own write.
class Num {
  1(): string {
    return "method 1";
  }
  2(): string {
    return "method 2";
  }
  one(): any {
    return this[1];
  }
}
function readTwo(x: Num): any {
  return x[2];
}
const n = new Num();
console.log("numeric before:", n[1](), typeof n[2]);
(n as any)[1] = "own index";
console.log("numeric after:", n[1], typeof n[2], n[2](), n.one(), typeof readTwo(n));

// A lone-surrogate name: an own property of that name is read back as stored.
// (Calling a class method declared with a lone-surrogate name is a separate,
// pre-existing gap: the method is registered under a lossy name.)
class Sur {
  ["\uD800"](): string {
    return "surrogate method";
  }
}
const s = new Sur();
(s as any)["\uD800"] = "own surrogate";
console.log("surrogate own:", s["\uD800"], (s as any)["\uD800"]);

// Method values keep identity across instances.
const x1 = new Base();
const x2 = new Base();
console.log("identity:", x1.m === x2.m, x1.m === Base.prototype.m);
