// THR3: an instance frozen after construction: method calls work, a strict
// field store throws.
class F {
  k: number;
  constructor() {
    Object.defineProperty(this, "id", { value: 1 });
    this.k = 2;
  }
  get2(): number {
    return this.k + (this as any).id;
  }
  set(v: number): string {
    try {
      this.k = v;
      return "stored";
    } catch (e) {
      return (e as Error).constructor.name;
    }
  }
}
const f = Object.freeze(new F());
const g = Object.seal(new F());
console.log(f.get2(), f.set(9), f.k, Object.isFrozen(f));
console.log(g.get2(), g.set(9), g.k, Object.isSealed(g), Object.isFrozen(g));
