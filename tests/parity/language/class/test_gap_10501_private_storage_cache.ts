// Fresh evaluations share a template but must retain distinct private state.
function makeClass(initial: number) {
  return class Cached {
    #value = initial;
    #bump(delta: number) { this.#value += delta; }
    step(delta: number) { this.#bump(delta); return this.#value; }
    read(other: any) { return other.#value; }
  };
}
const A = makeClass(10);
const B = makeClass(100);
const a: any = new A();
const b: any = new B();
let total = 0;
for (let i = 0; i < 200; i++) {
  a['key' + i] = i; // Transition receiver shapes, including overflow storage.
  total += a.step(1) + b.step(2);
}
console.log(total, a.step(0), b.step(0));
try { a.read(b); } catch (e) { console.log(e instanceof TypeError); }
try { a.read(Object.create(A.prototype)); } catch (e) { console.log(e instanceof TypeError); }
// Exercise eviction while older instances stay live.
let sum = 0;
for (let i = 0; i < 300; i++) {
  const C = makeClass(i);
  sum += new C().step(1);
}
console.log(sum, a.step(1), b.step(1));
// Ordinary template storage must retain heap values across cached writes.
class PlainCache {
  #value: any = { n: 0 };
  step(n: number) {
    this.#value = { n: this.#value.n + n };
    return this.#value.n;
  }
}
const plain: any = new PlainCache();
for (let i = 0; i < 200; i++) {
  plain['public' + i] = i;
  plain.step(1);
}
console.log(plain.step(1));
