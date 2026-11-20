// #11769: a method call on an instance of a per-evaluation class (a mixin
// `B => class extends B { … }`) is answered from that evaluation's
// prototype. Two evaluations of one class expression share the compiled
// method bodies, and so the call sites inside them, but each has its own
// prototype holding its own function objects. One call site that sees both
// evaluations alternately must call each receiver's own method: its
// evaluation's captured values, its private fields, its `super`, and a
// method replaced on one evaluation's prototype only.

class Base {
  n = 0;
  who(): string {
    return "base";
  }
  // A site in a shared class whose receivers are instances of both
  // evaluations.
  callWho(): string {
    return (this as any).who();
  }
}

const Mix = (B: typeof Base, tag: string, scale: number) =>
  class extends B {
    #secret = tag + "#";
    who(): string {
      return tag + "(" + super.who() + ")";
    }
    step(x: number): number {
      this.n += x * scale;
      return this.n;
    }
    reveal(): string {
      return this.#secret;
    }
    // Sites inside the class body: every evaluation runs this same compiled
    // code.
    twice(): string {
      return this.who() + "," + this.who();
    }
    run(k: number): number {
      let s = 0;
      for (let i = 0; i < k; i++) s += this.step(i);
      return s;
    }
  };

const A = Mix(Base, "a", 1);
const B = Mix(Base, "b", 10);
const a = new A();
const b = new B();
const a2 = new A();

// One call site, receivers of both evaluations alternately.
function callAll(objs: any[], rounds: number): string[] {
  const out: string[] = [];
  for (let r = 0; r < rounds; r++) {
    for (const o of objs) out.push(o.twice() + "|" + o.callWho() + "|" + o.reveal());
  }
  return out;
}
const first = callAll([a, b, a2, b], 3);
console.log(first.slice(0, 4).join(" "));
console.log(new Set(first).size);

let sa = 0;
let sb = 0;
for (let r = 0; r < 200; r++) {
  sa += a.run(5);
  sb += b.run(5);
}
console.log(sa, sb, a.n, b.n, a2.n);

// Replace a method on one evaluation's prototype only: the other keeps its
// own, and the replaced one is seen at once by every site.
(A.prototype as any).who = function (this: any): string {
  return "A-replaced:" + this.n;
};
console.log(callAll([a, b], 2).join(" "));

// Restore a body through `super`'s own class and add a third evaluation
// after the sites have learned the first two.
delete (A.prototype as any).who;
const C = Mix(Base, "c", 100);
const c = new C();
console.log(callAll([a, b, c], 2).join(" "));
console.log(c.run(3), a.run(1), b.run(1));

// An own property on one instance shadows its prototype's method.
(b as any).who = () => "own-b";
console.log(callAll([b, a, c], 1).join(" "));

console.log(Object.getPrototypeOf(a) === A.prototype, Object.getPrototypeOf(b) === B.prototype);
console.log(Object.getPrototypeOf(A.prototype) === Base.prototype, A === B);
