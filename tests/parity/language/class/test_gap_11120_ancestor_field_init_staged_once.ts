// #11120: every class's field initializers — `#private` ones included — must
// run exactly once per construction, whichever path reaches the parent
// constructor. Perry used to stage the chain root's initializers up front AND
// let the delegated parent constructor install them again, which threw
// "Cannot initialize a private field twice on the same object" (redis@6.1.0
// `createClient({ socket })`, from @redis/client's linked-list.js) or ran
// public initializers twice.

let n = 0;
const tick = (tag: string) => `${tag}${++n}`;

class Root {
  #count = 0;
  r = tick("r");
  get count() { return this.#count; }
  add() { ++this.#count; return this; }
  static has(o: object) { return #count in o; }
}

// 1. The issue's shape: a class declared where it captures an outer binding
//    (a CommonJS module body is exactly that) gets a synthesized constructor
//    whose `super(...args)` is spread.
function captureShape(make: () => unknown) {
  class Leaf extends Root { events = make(); l = tick("l"); }
  return Leaf;
}
const CapLeaf = captureShape(() => ({ kind: "emitter" }));
n = 0;
const a = new CapLeaf();
a.add();
console.log("capture", a.count, a.r, a.l, (a.events as any).kind, n);
n = 0;
const b = new CapLeaf();
console.log("capture again", b.count, b.r, b.l, n);

// 2. An explicit spread super.
class Spread extends Root {
  #own = "own";
  s = tick("s");
  constructor(...args: unknown[]) { super(...args); }
  get own() { return this.#own; }
}
n = 0;
const s = new Spread(1, 2);
console.log("spread", s.add().count, s.own, s.r, s.s, n);

// 3. A no-own-ctor subclass of a constructor-owning derived class, constructed
//    directly and through a class value.
class Mid extends Root {
  #m = "m";
  mid = tick("mid");
  constructor() { super(); }
  get m() { return this.#m; }
}
class Between extends Mid { between = tick("between"); }
class Tail extends Between { tail = tick("tail"); }
n = 0;
const t = new Tail();
console.log("inherited", t.add().count, t.m, t.r, t.mid, t.between, t.tail, n);
const TailValue: any = Tail;
n = 0;
const tv = new TailValue();
console.log("inherited dynamic", tv.add().count, tv.m, tv.r, tv.mid, tv.between, tv.tail, n);

// 4. The spread-super class as an inherited constructor, and captured.
class OverSpread extends Spread { o = tick("o"); }
n = 0;
const os = new OverSpread();
console.log("over spread", os.count, os.own, os.r, os.s, os.o, n);
function captureOver(tag: string) {
  class Cap extends Spread { c = tag; }
  return Cap;
}
const CapOver = captureOver("cap");
n = 0;
const co = new CapOver();
console.log("captured over spread", co.add().count, co.own, co.r, co.s, co.c, n);

// Brand checks still see one brand per instance.
console.log("brands", Root.has(a), Root.has(s), Root.has(tv), Root.has(co), Root.has({}));
