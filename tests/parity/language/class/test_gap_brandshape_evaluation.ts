// Birth evaluation survives prototype edits; foreign receivers stay unbranded.
function make(value: number) {
  return class {
    #value = value;
    #method() { return this.#value; }
    static #static = value;
    static has(o: any) { return #value in o && #method in o; }
    static read(o: any) { return o.#method(); }
    static readStatic(o: any) { return o.#static; }
    *values() { yield this.#method(); yield this.#value + 1; }
  };
}
const A = make(11), B = make(22);
const a: any = new A(), a2: any = new A(), b: any = new B();
console.log("evaluations", A.has(a), A.has(a2), A.has(b), B.has(a), B.has(b));
console.log("values", A.read(a), B.read(b), A.readStatic(A), B.readStatic(B));
for (const o of [{}, Object.create(A.prototype), new Proxy(a, {})]) {
  console.log("foreign", A.has(o));
  try { A.read(o); } catch (e: any) { console.log(e.constructor.name); }
}
try { A.read(b); } catch (e: any) { console.log("wrong evaluation", e.constructor.name); }
try { A.readStatic(B); } catch (e: any) { console.log("wrong static", e.constructor.name); }
Object.setPrototypeOf(a, null);
console.log("prototype changed", A.has(a), B.has(a), A.read(a));
const gen = A.prototype.values.call(a);
console.log("generator", gen.next().value, gen.next().value, gen.next().done);
class Sub extends A { #own = 33; static readOwn(o: any) { return o.#own; } }
const sub = new Sub();
console.log("subclass", A.has(sub), B.has(sub), A.read(sub), Sub.readOwn(sub));
function stamper(Base: any, value: number) {
  return class extends Base {
    constructor(o: any) { super(o); }
    #stamp = value;
    #method() { return this.#stamp; }
    static has(o: any) { return #stamp in o && #method in o; }
    static read(o: any) { return o.#method(); }
  };
}
class ReturnObject { constructor(o: any) { return o; } }
const S = stamper(ReturnObject, 44), T = stamper(ReturnObject, 55);
const o: any = { key: 1 };
new S(o); new T(o);
console.log("replacement", S.has(o), T.has(o), S.read(o), T.read(o), JSON.stringify(o));
try { new S(o); } catch (e: any) { console.log("duplicate", e.constructor.name); }
const prototype: any = A.prototype;
new T(prototype);
console.log("prototype stamped", T.has(prototype), T.read(prototype), A.has(a2), A.read(a2));
