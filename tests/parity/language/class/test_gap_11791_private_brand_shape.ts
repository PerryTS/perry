// #11791: private brands and private fields are facts of the receiver's shape.
// Fields, methods, accessors, statics, `#x in o`, subclasses branded only after
// super() returns, separate evaluations, reflection and JSON all match node.

class Counter {
  #n = 0;
  #step = 1;
  label = "c";
  #bump(by: number) { this.#n += by * this.#step; return this.#n; }
  get #double() { return this.#n * 2; }
  set #double(v: number) { this.#n = v / 2; }
  static #created = 0;
  static #tally() { return ++Counter.#created; }
  constructor() { Counter.#tally(); }
  run(k: number) { for (let i = 0; i < k; i++) this.#bump(i); return this.#n; }
  twice() { return this.#double; }
  setTwice(v: number) { this.#double = v; return this.#n; }
  static has(o: any) { return #n in o; }
  static hasMethod(o: any) { return #bump in o; }
  static hasAccessor(o: any) { return #double in o; }
  static created() { return Counter.#created; }
}

const c = new Counter();
console.log("run:", c.run(10), c.twice(), c.setTwice(40), c.run(1));
console.log("has:", Counter.has(c), Counter.hasMethod(c), Counter.hasAccessor(c));
console.log("foreign:", Counter.has({}), Counter.hasMethod({ label: "c" }), Counter.hasAccessor([]));
console.log("proto:", Counter.has(Counter.prototype), Counter.hasMethod(Object.create(Counter.prototype)));
new Counter();
console.log("statics:", Counter.created());

// Reflection never shows a private element.
console.log("keys:", Object.keys(c), Object.getOwnPropertyNames(c), Reflect.ownKeys(c));
console.log("json:", JSON.stringify(c), Object.entries(c), { ...c });
console.log("in:", "#n" in c, Object.hasOwn(c, "#n"), c.hasOwnProperty("#bump"));
console.log("descriptors:", Object.getOwnPropertyDescriptors(c));
const copy = Object.assign({}, c);
console.log("assign:", copy, Counter.has(copy));
for (const k in c) console.log("for-in:", k);

// A derived class brands its instance after super() returns.
let seen: string[] = [];
class Base {
  constructor() { seen.push(`during super: ${(this as any).probe()}`); }
}
class Derived extends Base {
  #f = 1;
  #m() { return 2; }
  probe() { return `${#f in this} ${#m in this}`; }
  static check(o: any) { return `${#f in o} ${#m in o}`; }
}
const d = new Derived();
console.log(seen.join(", "), "after:", Derived.check(d), d.probe());

// Brands of every class in the chain, each checked by its own class.
class A1 { #a = "a"; static has(o: any) { return #a in o; } getA() { return this.#a; } }
class B1 extends A1 { #b() { return "b"; } static has(o: any) { return #b in o; } getB() { return this.#b(); } }
class C1 extends B1 { #c = "c"; static has(o: any) { return #c in o; } getC() { return this.#c; } }
const c1 = new C1(), b1 = new B1(), a1 = new A1();
console.log("chain:", A1.has(c1), B1.has(c1), C1.has(c1), c1.getA() + c1.getB() + c1.getC());
console.log("chain partial:", A1.has(b1), B1.has(b1), C1.has(b1), A1.has(a1), B1.has(a1));

// Two evaluations of one class body brand differently.
function make(tag: string) {
  return class {
    #y = tag;
    #who() { return this.#y; }
    static has(o: any) { return #y in o; }
    static hasM(o: any) { return #who in o; }
    read(o: any) { return o.#who(); }
  };
}
const K1 = make("one"), K2 = make("two");
const k1 = new K1(), k2 = new K2();
console.log("evals:", K1.has(k1), K1.has(k2), K2.has(k2), K2.has(k1), K1.hasM(k2), K2.hasM(k2));
console.log("eval read:", k1.read(k1), k2.read(k2));
try { k1.read(k2); } catch (e: any) { console.log(e.constructor.name + ": " + e.message); }

// Adding public keys and deleting them keeps the brand and the fields.
const grow: any = new Counter();
grow.extra = 1;
grow.more = 2;
delete grow.extra;
Object.defineProperty(grow, "hidden", { value: 3, enumerable: false });
console.log("after edits:", Counter.has(grow), Counter.hasMethod(grow), grow.run(3), Object.keys(grow));
for (let i = 0; i < 40; i++) grow["k" + i] = i;
for (let i = 0; i < 40; i += 2) delete grow["k" + i];
console.log("dictionary:", Counter.has(grow), Counter.hasMethod(grow), grow.run(2), Object.keys(grow).length);
Object.setPrototypeOf(grow, null);
console.log("reparented:", Counter.has(grow), Counter.hasMethod(grow));

// freeze does not freeze private fields.
class Frozen { #v = 1; bump() { return ++this.#v; } constructor() { Object.freeze(this); } }
const fr = new Frozen();
console.log("frozen:", fr.bump(), fr.bump(), Object.isFrozen(fr));
