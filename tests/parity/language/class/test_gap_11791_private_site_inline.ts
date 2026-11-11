// #11791: compiled private-access sites answer from an inline ShapeId compare
// and call a private method's body directly. A primed site must still reject
// every receiver that lacks the element, a second evaluation of the class must
// turn the shortcut off, and a direct call must bind `this` and its arguments
// exactly like the generic call.

class Box {
  #v = 1;
  #tag = "box";
  #add(x: number, y: number = 10) { this.#v += x + y; return this.#v; }
  #self() { return this; }
  #throws(msg: string) { throw new Error(msg); }
  static read(o: any) { return o.#v; }
  static write(o: any, v: number) { o.#v = v; return v; }
  static call(o: any, x: number) { return o.#add(x, 0); }
  static callDefault(o: any) { return o.#add(1); }
  static callExtra(o: any) { return (o.#add as any)(1, 2, 3, 4); }
  static self(o: any) { return o.#self() === o; }
  static boom(o: any) { return o.#throws("inside"); }
  static tag(o: any) { return o.#tag; }
}

function attempt(label: string, f: () => unknown) {
  try {
    console.log(label, f());
  } catch (e: any) {
    console.log(label, e.constructor.name + ": " + e.message);
  }
}

const b = new Box();
let sum = 0;
for (let i = 0; i < 200; i++) {
  sum += Box.read(b) + Box.write(b, i) + Box.call(b, 1);
}
console.log("primed:", sum, Box.read(b), Box.tag(b), Box.callDefault(b), Box.callExtra(b), Box.self(b));

// Every primed site must still reject a receiver without the element: a plain
// object, an object with the same public keys, an array, a function, a
// prototype, a Box-like class instance, and primitives.
class Lookalike { v = 1; tag = "box"; }
const foreign: [string, any][] = [
  ["plain", {}],
  ["keys", { v: 1, tag: "box" }],
  ["array", [1, 2, 3]],
  ["function", function () {}],
  ["proto", Box.prototype],
  ["lookalike", new Lookalike()],
  ["created", Object.create(b)],
  ["number", 5],
  ["string", "box"],
  ["undefined", undefined],
];
for (const [label, o] of foreign) {
  attempt("read " + label + ":", () => Box.read(o));
  attempt("write " + label + ":", () => Box.write(o, 1));
  attempt("call " + label + ":", () => Box.call(o, 1));
}
// The real instance still hits after the misses.
console.log("again:", Box.read(b), Box.call(b, 0));

// A throw inside a directly called private method unwinds normally.
attempt("boom:", () => Box.boom(b));
console.log("after boom:", Box.read(b));

// Receivers that reach the same sites on different shapes all keep working.
const shapes: any[] = [];
for (let i = 0; i < 6; i++) {
  const o: any = new Box();
  for (let k = 0; k < i; k++) o["p" + k] = k;
  shapes.push(o);
}
let total = 0;
for (let r = 0; r < 50; r++) for (const o of shapes) total += Box.call(o, 1) + Box.read(o);
console.log("shapes:", total);

// A second evaluation of a class body: its sites must reject the first
// evaluation's instances (and vice versa) even after they were primed.
function make(tag: string) {
  return class {
    #y = tag;
    #who(suffix: string) { return this.#y + suffix; }
    read(o: any) { return o.#y; }
    who(o: any) { return o.#who("!"); }
  };
}
const K1 = make("one");
const k1 = new K1();
let primed = "";
for (let i = 0; i < 100; i++) primed = k1.read(k1) + k1.who(k1);
console.log("k1 primed:", primed);
const K2 = make("two");
const k2 = new K2();
for (let i = 0; i < 100; i++) primed = k2.read(k2) + k2.who(k2);
console.log("k2 primed:", primed, k1.read(k1), k1.who(k1));
attempt("k1 reads k2:", () => k1.read(k2));
attempt("k2 reads k1:", () => k2.read(k1));
attempt("k1 calls k2:", () => k1.who(k2));
attempt("k2 calls k1:", () => k2.who(k1));

// An argument that evaluates the class again between the brand check and the
// call: the call must still reach the right evaluation's method.
function makeCounter() {
  return class Counter {
    #n = 0;
    #inc(by: number) { this.#n += by; return this.#n; }
    run(by: number) { return this.#inc(by); }
    runWith(f: () => number) { return this.#inc(f()); }
  };
}
const C1 = makeCounter();
const c1 = new C1();
for (let i = 0; i < 100; i++) c1.run(1);
let made: any = null;
console.log("mid-call eval:", c1.runWith(() => { made = makeCounter(); return 5; }), new made().run(2), c1.run(1));

// Static private fields stay invisible to reflection on the class.
class Statics {
  static #count = 3;
  static visible = 1;
  static bump() { return ++Statics.#count; }
}
console.log("statics:", Statics.bump(), Object.getOwnPropertyNames(Statics).sort().join(","), Object.keys(Statics).join(","));
function makeStatics() {
  return class {
    static #hidden = 7;
    static shown = 2;
    static peek() { return this.#hidden; }
  };
}
const S1 = makeStatics(), S2 = makeStatics();
console.log("fresh statics:", S1.peek(), S2.peek(), Object.getOwnPropertyNames(S2).sort().join(","), JSON.stringify(Object.entries(S2)));

// Public fields of a class with private elements: no finished instance is on
// the class's birth shape, so their reads and writes go through the generic
// property ICs. Subclass instances, extra keys, a frozen instance and a
// second evaluation must all read and write correctly through the same sites.
class Account {
  balance = 0;
  owner = "a";
  #audit = 0;
  #log(x: number) { this.#audit += x; }
  deposit(x: number) { this.#log(x); this.balance = this.balance + x; return this.balance; }
  audit() { return this.#audit; }
}
class Savings extends Account {
  rate = 2;
  #bonus = 1;
  grow() { this.balance = this.balance * this.rate + this.#bonus; return this.balance; }
}
const acct = new Account();
const sav = new Savings();
const wide: any = new Account();
wide.extra = 1;
let dep = 0;
for (let i = 0; i < 100; i++) dep += acct.deposit(1) + sav.deposit(2) + wide.deposit(3);
console.log("public fields:", dep, acct.balance, sav.balance, wide.balance, sav.grow(), acct.audit(), sav.audit());
const frozen = new Account();
frozen.deposit(5);
Object.freeze(frozen);
attempt("frozen deposit:", () => frozen.deposit(1));
console.log("frozen after:", frozen.balance, frozen.audit());
function makeAccount() {
  return class {
    total = 0;
    #seen = 0;
    add(x: number) { this.#seen++; this.total = this.total + x; return this.total + this.#seen; }
  };
}
const A1 = makeAccount(), A2 = makeAccount();
const a1 = new A1(), a2 = new A2();
let adds = 0;
for (let i = 0; i < 50; i++) adds += a1.add(1) + a2.add(2);
console.log("evaluations:", adds, a1.total, a2.total, JSON.stringify(a1), Object.keys(sav).join(","));
