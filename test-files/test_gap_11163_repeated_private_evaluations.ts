function make(Base: any, tag: string) {
  class C extends Base {
    #v: string;
    constructor() { super(); this.#v = tag; }
    get() { return this.#v; }
    static has(o: any) { return #v in o; }
    static read(o: any) { return o.#v; }
  }
  return C;
}
class Root {}
const C1: any = make(Root, "first");
const C2: any = make(C1, "second");
const x = new C2();
console.log("own", x.get());
console.log("C1 method on x", C1.prototype.get.call(x));
console.log("C2 method on x", C2.prototype.get.call(x));
console.log("static read", C1.read(x), C2.read(x));
console.log("has", C1.has(x), C2.has(x));
const y = new C1();
console.log("C1 only", y.get(), C1.has(y), C2.has(y));
try { console.log(C2.read(y)); } catch (e) { console.log("C2.read(y) threw", (e as Error).constructor.name); }
const C3: any = make(Root, "third");
console.log("unrelated", C3.has(x), C1.has(new C3()));

function richer(Base: any, value: number) {
  class Repeat extends Base {
    #value = value;
    #read() { return this.#value; }
    get #access() { return this.#value; }
    set #access(v: number) { this.#value = v; }
    read() { return this.#read(); }
    extract() { return this.#read; }
    increment() { return ++this.#access; }
    static read(o: any) { return o.#access; }
    static relay(o: Repeat) { return o.read(); }
  }
  return Repeat;
}
const R1: any = richer(Root, 10);
const R2: any = richer(R1, 20);
const r = new R2();
console.log('private calls', R1.prototype.read.call(r), R2.prototype.read.call(r));
console.log('private accessors', R1.prototype.increment.call(r), R2.prototype.increment.call(r));
console.log('private reads', R1.read(r), R2.read(r));
const m1 = R1.prototype.extract.call(r), m2 = R2.prototype.extract.call(r);
console.log('private extracted', m1.call(r), m2.call(r), m1 === m2);

console.log("nested receiver", R1.relay(r), R2.relay(r));
