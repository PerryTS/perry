// #11791: `this.#x` / `local.#x` compile to a shape compare and the field's
// slot; the slot's lane follows the stored values (F64 for numbers, Any after
// anything else). Every path here must print exactly what node prints.

class Counter {
  #n = 0;
  #label: any = "c";
  add(x: any) {
    this.#n = this.#n + x;
    return this.#n;
  }
  setLabel(v: any) {
    this.#label = v;
    return this.#label;
  }
  static peek(o: Counter) {
    return o.#n;
  }
  static poke(o: any, v: any) {
    o.#n = v;
    return o.#n;
  }
}

const cs: Counter[] = [];
for (let i = 0; i < 50; i++) cs.push(new Counter());
let sum = 0;
for (let r = 0; r < 200; r++) for (const c of cs) sum += c.add(r % 7);
console.log("sum", sum, Counter.peek(cs[3]));

// The numeric lane generalizes and keeps working.
console.log(
  "lane",
  Counter.poke(cs[0], "str"),
  cs[0].add(1),
  Counter.poke(cs[0], NaN),
  Counter.poke(cs[0], Infinity),
  Object.is(Counter.poke(cs[0], -0), -0),
);
console.log("other instances", cs[1].add(0.5), Counter.peek(cs[1]), cs[2].add(2 ** 40));

// Heap values in a private slot survive collections.
for (let i = 0; i < 5000; i++) cs[i % 50].setLabel({ i, s: "x" + i, arr: [i, i + 1] });
const kept = cs.map((c) => c.setLabel(c.setLabel(null) ?? { k: "v" }));
console.log("labels", JSON.stringify(kept.slice(0, 2)), JSON.stringify(cs[7].setLabel(["a", { b: 1 }])));
for (let i = 0; i < 50; i++) cs[i].add("s" + i);
console.log("strings", Counter.peek(cs[9]), Counter.peek(cs[49]));
// The strings now in former number slots must survive collections that move
// them: a slot still typed as a raw double would keep a stale pointer.
const junk: any[] = [];
for (let i = 0; i < 20000; i++) junk.push({ i, s: "j" + i });
for (let i = 0; i < 50; i++) cs[i].setLabel("l" + i + junk[i * 7].s);
console.log("strings after allocation", Counter.peek(cs[9]), Counter.peek(cs[49]), cs[3].setLabel(cs[3].add("!")), junk.length);

// Wrong receivers: the brand check, after the right-hand side.
try {
  Counter.peek({} as any);
} catch (e: any) {
  console.log(e.constructor.name, e.message);
}
const order: string[] = [];
try {
  Counter.poke({}, (order.push("rhs"), 1));
} catch (e: any) {
  order.push("throw");
  console.log(e.constructor.name, e.message);
}
console.log("order", order.join(","));

// A subclass instance reaches the declaring class's sites.
class Sub extends Counter {
  #m = 1;
  m() {
    return this.#m++;
  }
}
const s = new Sub();
s.add(5);
console.log("sub", Counter.peek(s), s.m(), s.m(), Counter.poke(s, 9), s.add(1));

// Fresh class evaluations get their own storage.
function make() {
  return class {
    #v = 1;
    inc() {
      return ++this.#v;
    }
    static get(o: any) {
      return o.#v;
    }
  };
}
const K1 = make();
const K2 = make();
const k1 = new K1();
const k2 = new K2();
console.log("fresh", k1.inc(), k2.inc(), k2.inc(), K1.get(k1), K2.get(k2));
try {
  K1.get(k2);
} catch (e: any) {
  console.log(e.constructor.name, e.message);
}

// A return override puts private fields on a foreign object.
class Base {
  constructor(o: any) {
    return o;
  }
}
class Stamp extends Base {
  #tag = 42;
  static tag(o: any) {
    return o.#tag;
  }
  static retag(o: any, v: any) {
    o.#tag = v;
    return o.#tag;
  }
}
const plain: any = { a: 1 };
new Stamp(plain);
console.log("override", Stamp.tag(plain), Stamp.retag(plain, "t"), JSON.stringify(plain), Object.keys(plain).join());

// A derived class's private field is absent while its base constructor runs.
class B2 {
  constructor() {
    (this as any).probe();
  }
}
class D2 extends B2 {
  #p = 3;
  probe() {
    try {
      return this.#p;
    } catch (e: any) {
      console.log("during super:", e.constructor.name, e.message);
    }
  }
}
console.log("after super", new D2().probe());

// Public and private fields together, past the inline floor.
class Wide {
  a = 1;
  b = 2;
  c = 3;
  #x = 4;
  #y = 5;
  #z = "z";
  sum() {
    return this.a + this.b + this.c + this.#x + this.#y + this.#z;
  }
  bump() {
    this.#x += 10;
    this.#y *= 2;
    this.a++;
    return this.sum();
  }
}
const ws: Wide[] = [];
for (let i = 0; i < 20; i++) ws.push(new Wide());
let wsum = "";
for (const w of ws) wsum = w.bump();
console.log("wide", ws[0].sum(), wsum, ws[3].bump(), Object.keys(ws[0]).join(), JSON.stringify(ws[0]));

// A class with only private methods keeps its public fields reachable.
class OnlyMethod {
  n = 0;
  #bump(x: number) {
    this.n = (this.n + x) | 0;
  }
  step(i: number) {
    this.#bump(i);
    return this.n;
  }
}
const om = new OnlyMethod();
let last = 0;
for (let i = 0; i < 1000; i++) last = om.step(i);
console.log("method", last, om.n, Object.keys(om).join());
