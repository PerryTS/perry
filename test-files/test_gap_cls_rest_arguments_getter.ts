// CLS D1: methods with rest / arguments, and a getter-valued method name,
// called on annotated and `this` receivers.
class Base {
  add(a: number, ...rest: number[]): number { return a + rest.length; }
  cnt(): number { return arguments.length; }
  run(): number { return this.add(1, 2, 3) + this.cnt(); }
}
class A extends Base {
  add(a: number, ...rest: number[]): number { return a * 10 + rest.length; }
}
class B extends Base {}
class G {
  get area(): () => number { return () => 42; }
  go(): number { return (this as any).area(); }
}
const xs: Base[] = [new A(), new B(), new A()];
let t = 0;
for (let i = 0; i < xs.length; i++) {
  const b: Base = xs[i];
  t += b.run() + b.add(5, 6) + b.cnt();
}
console.log(t);
const g: G = new G();
console.log(g.go(), (g as any).area());
