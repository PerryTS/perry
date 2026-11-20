// #10501: private member access uses per-site caches and a fused
// `recv.#m(args)` call. Every check below warms a site first and then feeds it
// a receiver the cache must NOT answer for.

function tryIt(label: string, fn: () => unknown): void {
  try {
    console.log(label, "ok", fn());
  } catch (e) {
    console.log(label, "threw", (e as Error).constructor.name);
  }
}

// --- fields: warm read/write sites, then a foreign receiver ----------------
class Counter {
  #n = 0;
  #step: number;
  constructor(step: number) {
    this.#step = step;
  }
  bump(): number {
    this.#n = this.#n + this.#step;
    return this.#n;
  }
  static read(o: any): number {
    return o.#n;
  }
  static write(o: any, v: number): number {
    o.#n = v;
    return o.#n;
  }
  static has(o: any): boolean {
    return #n in o;
  }
}

const counters = [new Counter(1), new Counter(2), new Counter(3)];
let total = 0;
for (let i = 0; i < 2000; i++) {
  total += counters[i % 3].bump();
}
console.log("counter total", total);
for (let i = 0; i < 1000; i++) Counter.read(counters[i % 3]);
tryIt("read plain object", () => Counter.read({}));
tryIt("read look-alike keys", () => Counter.read({ "#n": 1 }));
tryIt("write plain object", () => Counter.write({}, 5));
tryIt("read after warm", () => Counter.read(counters[0]));
console.log("has instance", Counter.has(counters[1]), "has plain", Counter.has({}));

// --- many private fields: markers past the inline slots --------------------
class Wide {
  #a = 1;
  #b = 2;
  #c = 3;
  #d = 4;
  #e = 5;
  #f = 6;
  #g = 7;
  #h = 8;
  sum(): number {
    return this.#a + this.#b + this.#c + this.#d + this.#e + this.#f + this.#g + this.#h;
  }
  shift(): void {
    const a = this.#a;
    this.#a = this.#b;
    this.#b = this.#c;
    this.#c = this.#d;
    this.#d = this.#e;
    this.#e = this.#f;
    this.#f = this.#g;
    this.#g = this.#h;
    this.#h = a;
  }
  static h(o: any): number {
    return o.#h;
  }
}
const wide = new Wide();
let wideAcc = 0;
for (let i = 0; i < 500; i++) {
  wide.shift();
  wideAcc += wide.sum() + Wide.h(wide);
}
console.log("wide", wideAcc, Wide.h(wide));
tryIt("wide foreign", () => Wide.h(new Counter(1)));

// --- methods: the fused call ------------------------------------------------
let sideEffects = 0;
function effect(): number {
  sideEffects++;
  return sideEffects;
}

class Calc {
  #base: number;
  constructor(base: number) {
    this.#base = base;
  }
  #add(x: number, y = 10): number {
    return this.#base + x + y;
  }
  #count(...rest: number[]): number {
    return rest.length;
  }
  #argc(): number {
    return arguments.length;
  }
  #self(): Calc {
    return this;
  }
  #fail(): never {
    throw new RangeError("boom");
  }
  run(i: number): number {
    return this.#add(i) + this.#add(i, 1) + this.#count(1, 2, 3) + this.#argc(1, 2);
  }
  identity(): boolean {
    return this.#self() === this;
  }
  failAndRecover(): number {
    try {
      this.#fail();
    } catch (e) {
      return this.#add((e as Error).message.length);
    }
    return -1;
  }
  static callOn(o: any): number {
    return o.#add(effect());
  }
}
const calc = new Calc(100);
let calcAcc = 0;
for (let i = 0; i < 1000; i++) calcAcc += calc.run(i);
console.log("calc", calcAcc, calc.identity(), calc.failAndRecover());
for (let i = 0; i < 100; i++) Calc.callOn(calc);
const before = sideEffects;
tryIt("method on plain object", () => Calc.callOn({}));
console.log("argument evaluated before brand check?", sideEffects !== before);
tryIt("method on other class", () => Calc.callOn(new Counter(1)));
tryIt("method after foreign", () => Calc.callOn(calc));

// --- methods that call each other, recursion, `this` of a subclass ---------
class Tree {
  #depth: number;
  constructor(depth: number) {
    this.#depth = depth;
  }
  #walk(n: number): number {
    return n <= 0 ? this.#depth : this.#walk(n - 1) + 1;
  }
  #twice(n: number): number {
    return this.#walk(n) * 2;
  }
  measure(n: number): number {
    return this.#twice(n);
  }
}
class Forest extends Tree {
  constructor() {
    super(7);
  }
  total(): number {
    return this.measure(3) + this.measure(4);
  }
}
let forestAcc = 0;
for (let i = 0; i < 300; i++) forestAcc += new Forest().total();
console.log("forest", forestAcc);

// --- a nested class reaching an outer private method -----------------------
class Outer {
  #secret(): string {
    return "outer-secret";
  }
  probe(): string[] {
    const self = this;
    class Inner {
      read(o: any): string {
        return o.#secret();
      }
    }
    const inner = new Inner();
    const out: string[] = [];
    for (let i = 0; i < 3; i++) out.push(inner.read(self));
    try {
      inner.read(inner);
    } catch (e) {
      out.push((e as Error).constructor.name);
    }
    return out;
  }
}
console.log("nested", new Outer().probe().join(","));

// --- fresh class evaluations keep distinct brands --------------------------
function makeClass(tag: string) {
  return class {
    #tag = tag;
    #label(): string {
      return "<" + this.#tag + ">";
    }
    label(): string {
      return this.#label();
    }
    peek(o: any): string {
      return o.#label();
    }
    tagOf(o: any): string {
      return o.#tag;
    }
  };
}
const A = makeClass("a");
const B = makeClass("b");
const a1 = new A();
const a2 = new A();
const b1 = new B();
let labels = "";
for (let i = 0; i < 200; i++) labels = a1.label() + a2.peek(a1) + a1.tagOf(a2) + b1.label();
console.log("fresh", labels);
tryIt("cross-evaluation method", () => a1.peek(b1));
tryIt("cross-evaluation field", () => b1.tagOf(a1));
tryIt("same evaluation after cross", () => a2.peek(a1));

// --- a constructor return override stamps private fields on a plain object -
class Base {
  constructor(o: object) {
    return o;
  }
}
class Stamp extends Base {
  #mark = "stamped";
  static read(o: any): string {
    return o.#mark;
  }
  static has(o: any): boolean {
    return #mark in o;
  }
}
const target = { visible: true };
new Stamp(target);
for (let i = 0; i < 100; i++) Stamp.read(target);
console.log("stamp", Stamp.read(target), Stamp.has(target), Stamp.has({ visible: true }));
tryIt("stamp twice", () => new Stamp(target));
tryIt("stamp look-alike", () => Stamp.read({ visible: true }));

// --- async and generator private methods -----------------------------------
class Streamer {
  #items = [1, 2, 3];
  *#each(): Generator<number> {
    for (const item of this.#items) yield item * 10;
  }
  async #load(n: number): Promise<number> {
    await null;
    return n + this.#items.length;
  }
  collect(): number[] {
    return [...this.#each()];
  }
  async total(): Promise<number> {
    let sum = 0;
    for (let i = 0; i < 5; i++) sum += await this.#load(i);
    return sum;
  }
}
const streamer = new Streamer();
console.log("gen", streamer.collect().join(","));
streamer.total().then((v) => console.log("async", v));
