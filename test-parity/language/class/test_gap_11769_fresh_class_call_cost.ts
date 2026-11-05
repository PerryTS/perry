// #11769 / #11932: `this.step(i)` on an instance of a per-evaluation class
// (prettier's babel parser is built from mixins, so every parser class is
// one) repeated many times must give the same answer as a class declared
// once. The method is answered from the evaluation's prototype by its
// ShapeId; this pins the result of the hot loop the cost measurement uses.

class Base {
  n = 0;
}
const mk = (B: typeof Base) =>
  class extends B {
    step(x: number): number {
      this.n += x;
      return this.n;
    }
    run(N: number): number {
      let s = 0;
      for (let i = 0; i < N; i++) s += this.step(i);
      return s;
    }
  };

class S extends Base {
  step(x: number): number {
    this.n += x;
    return this.n;
  }
  run(N: number): number {
    let s = 0;
    for (let i = 0; i < N; i++) s += this.step(i);
    return s;
  }
}

const F = mk(Base);
const G = mk(Base);
const f = new F();
const g = new G();
const s = new S();
console.log(f.run(20000), g.run(30000), s.run(20000));
console.log(f.n, g.n, s.n);
for (let r = 0; r < 4; r++) console.log(f.run(1000 + r), g.run(1000 - r));
