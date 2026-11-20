// E11: Reflect.construct with a different newTarget.
// WPFACTS: fact F3 prefix_stable NT = 0
class Base1 {
  b: number = 1;
}
class NT {
  n: number = 2;
  show() { return "n=" + this.n; }
}
const x: any = Reflect.construct(Base1, [], NT);
console.log(x instanceof NT, x.n, x.show());
