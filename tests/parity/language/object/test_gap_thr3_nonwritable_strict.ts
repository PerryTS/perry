// THR3: a strict store to a constructor-defined non-writable key throws a
// TypeError and leaves the value; a store compiled for the family layout
// must not write the slot raw.
class A {
  k: number;
  constructor(n: number) {
    Object.defineProperty(this, "id", { value: n });
    this.k = 2;
  }
  bump(): string {
    try {
      (this as any).id = 5;
      return "stored";
    } catch (e) {
      return (e as Error).constructor.name;
    }
  }
}
class B extends A {
  constructor(n: number) {
    super(n);
  }
  again(): string {
    try {
      (this as any).id = 9;
      return "stored";
    } catch (e) {
      return (e as Error).constructor.name;
    }
  }
}
const out: string[] = [];
for (let i = 0; i < 3; i++) {
  const a = new A(i + 1);
  out.push(a.bump() + ":" + (a as any).id);
  const b = new B(i + 10);
  out.push(b.again() + ":" + (b as any).id + ":" + b.k);
}
console.log(out.join(" "));
