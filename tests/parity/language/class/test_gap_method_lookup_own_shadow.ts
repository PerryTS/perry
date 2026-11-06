// A class-method value read checks for an own data property of the same name
// first ([[Get]] order) and only then falls back to the shared prototype
// method. The own lookup compares the method name's bytes against the keys
// directly; it must keep every shadowing case and every identity Node has,
// for static names, computed keys and the `this.m = this.m.bind(this)` idiom.

class Base {
  tag: string;
  constructor(tag: string) {
    this.tag = tag;
    this.parse = this.parse.bind(this);
    this.safeParse = this.safeParse.bind(this);
  }
  parse(x: number): string {
    return this.tag + ":parse:" + x;
  }
  safeParse(x: number): string {
    return this.tag + ":safe:" + x;
  }
  plain(): string {
    return this.tag + ":plain";
  }
}

class Derived extends Base {
  constructor(tag: string) {
    super(tag);
  }
  extra(): string {
    return "extra:" + this.tag;
  }
}

const a = new Base("a");
const b = new Base("b");
console.log(a.parse(1), b.parse(2), a.safeParse(3));
console.log(a.parse === b.parse, a.plain === b.plain, a.plain === Base.prototype.plain);
console.log(a.parse === Base.prototype.parse, Object.prototype.hasOwnProperty.call(a, "parse"));

// The bound own property survives detaching.
const detached = a.parse;
console.log(detached(7), detached === a.parse);

// Computed keys see the same own-first order.
const names = ["parse", "safeParse", "plain", "missing"];
for (const name of names) {
  const value = (a as any)[name];
  console.log(name, typeof value, typeof value === "function" ? value.call(b, 9) : String(value));
}

// An own property that is not a function shadows the method too.
const c: any = new Base("c");
c.plain = "own-string";
console.log(typeof c.plain, c.plain);
c.plain = undefined;
console.log(typeof c.plain);
delete c.plain;
console.log(typeof c.plain, c.plain());

// Subclass instances: the constructor-installed bound methods are own.
const d = new Derived("d");
console.log(d.parse(4), d.extra(), d.parse === a.parse);
const extraKey = "ex" + "tra";
console.log((d as any)[extraKey]() , (d as any)[extraKey] === Derived.prototype.extra);

// Many instances in a loop: each keeps its own bound method.
let acc = 0;
const seen = new Set<unknown>();
for (let i = 0; i < 2000; i++) {
  const inst = new Derived("i" + i);
  seen.add(inst.parse);
  if (inst.parse(i) === "i" + i + ":parse:" + i) acc++;
}
console.log(acc, seen.size);

// Shadowing by an own accessor installed with defineProperty.
const e: any = new Base("e");
Object.defineProperty(e, "plain", { get: () => () => "from-getter", configurable: true });
console.log(e.plain());

// Long and unicode method names.
class Names {
  ["veryLongMethodNameThatIsDefinitelyLongerThanAShortStringSlot"](): string {
    return "long";
  }
  ["métho∂"](): string {
    return "unicode";
  }
}
const n: any = new Names();
console.log(n.veryLongMethodNameThatIsDefinitelyLongerThanAShortStringSlot(), n["métho∂"]());
n["métho∂"] = () => "own-unicode";
n.veryLongMethodNameThatIsDefinitelyLongerThanAShortStringSlot = () => "own-long";
console.log(n.veryLongMethodNameThatIsDefinitelyLongerThanAShortStringSlot(), n["métho∂"]());
