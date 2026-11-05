// #11127: a base class with a #private field and a subclass, both declared
// inside a function, are per-evaluation classes. An inherited base method
// (not only a getter) must read and write the base's private elements on a
// subclass instance.

function local() {
  class S {
    #l = 0;
    get l() { return this.#l; }
    read() { return this.#l; }
    inc() { ++this.#l; }
    addTo(n: number) { this.#l += n; return this.#l; }
    has(o: any) { return #l in o; }
    #hidden() { return "hidden:" + this.#l; }
    callHidden() { return this.#hidden(); }
  }
  class E extends S { x = 1; }
  class F extends E { y = 2; }
  const a = new E();
  console.log("getter", a.l);
  console.log("read", a.read());
  a.inc();
  console.log("after inc", a.l, a.read());
  console.log("addTo", a.addTo(5));
  console.log("brand check", a.has(a), a.has({}));
  console.log("private method", a.callHidden());
  const b = new F();
  b.inc();
  b.inc();
  console.log("grandchild", b.read(), b.x, b.y, b.has(b));
  const s = new S();
  s.inc();
  console.log("base", s.read(), s.has(a), a.has(s));
  return { S, E, a };
}
const first = local();
const second = local();
// A second evaluation's methods must NOT accept the first evaluation's
// instances: each evaluation has its own private names.
console.log("same eval", first.a.read(), second.a.read());
console.log("cross-eval brand", second.a.has(first.a), first.a.has(second.a));
try {
  console.log("cross-eval read", second.S.prototype.read.call(first.a));
} catch (e) {
  console.log("cross-eval read threw", (e as Error).constructor.name);
}

// A factory whose class expression extends a function-local base.
function makePair(start: number) {
  class Counter {
    #n: number;
    constructor() { this.#n = start; }
    next() { return this.#n++; }
  }
  return class extends Counter {
    twice() { return [this.next(), this.next()]; }
  };
}
const P = makePair(10);
const p = new P();
console.log("factory", p.twice().join(","), p.next());
