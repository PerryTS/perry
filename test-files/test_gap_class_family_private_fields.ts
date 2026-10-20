// Private fields of a static class family (object model step 8): the
// element is part of the family layout, absent until its definition fills
// the slot. Brand checks, `#x in o`, enumeration and same-spelled public
// keys must behave exactly as without the layout.

class Base {
  kind = "base";
  #secret: number;
  #label = "L";
  constructor(n: number) {
    this.#secret = n;
  }
  secret(): number {
    return this.#secret;
  }
  label(): string {
    return this.#label;
  }
  setSecret(v: number): void {
    this.#secret = v;
  }
  static has(o: object): boolean {
    return #secret in o;
  }
  static read(o: any): number {
    return o.#secret;
  }
}

const a = new Base(1);
const b = new Base(2);
console.log(a.secret(), b.secret(), a.label(), b.kind);
a.setSecret(10);
console.log(a.secret(), b.secret());
console.log(Base.has(a), Base.has({}), Base.has(b));
console.log(Object.keys(a).join(","), JSON.stringify(a));
console.log(Object.getOwnPropertyNames(a).join(","), Reflect.ownKeys(b).length);
console.log(JSON.stringify({ ...a }), Object.assign({}, b).kind);
try {
  Base.read({ kind: "base" });
} catch (e) {
  console.log("read plain:", e instanceof TypeError);
}

// A public key spelled like the runtime's private storage name is a
// different key.
const spelled = Object.keys(Object.getOwnPropertyDescriptors(a));
console.log(spelled.length);
(a as any)["#secret"] = "public";
console.log(a.secret(), (a as any)["#secret"], Object.keys(a).join(","));

// A parent constructor that reads a subclass's private field before the
// subclass defined it: the element is still a hole, so it is absent.
class Parent {
  seen: string;
  constructor() {
    this.seen = this.peek();
  }
  peek(): string {
    return "parent";
  }
}
class Child extends Parent {
  #x = 5;
  extra = 1;
  peek(): string {
    try {
      return "read " + this.#x;
    } catch (e) {
      return "absent " + (e instanceof TypeError);
    }
  }
  has(): boolean {
    return #x in this;
  }
  x(): number {
    return this.#x;
  }
}
const c = new Child();
console.log(c.seen, c.x(), c.has(), Object.keys(c).join(","));

// Defining a private field on another object through a constructor
// return override, then twice.
class Stamper {
  constructor(o: object) {
    return o;
  }
}
class Over extends Stamper {
  #own = 1;
  constructor(o: object) {
    super(o);
  }
  static own(o: any): number {
    return o.#own;
  }
}
const target = new Base(3);
new Over(target);
console.log(Over.own(target), target.secret(), Object.keys(target).join(","));
try {
  new Over(target);
} catch (e) {
  console.log("twice:", e instanceof TypeError);
}

// Many instances: the element of each is its own.
const many: Base[] = [];
for (let i = 0; i < 1000; i++) many.push(new Base(i));
let sum = 0;
for (const m of many) sum += m.secret();
console.log(sum, many[999].label());

// Frozen instances still get their private fields defined.
class Frozen {
  #v: number;
  constructor() {
    Object.freeze(this);
    this.#v = 7;
  }
  v(): number {
    return this.#v;
  }
}
console.log(new Frozen().v());
