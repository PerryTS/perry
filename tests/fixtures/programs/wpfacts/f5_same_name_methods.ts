// F5: two classes define a method of the same name; a call site that sees
// both receivers has two targets, so it is not monomorphic.
// WPFACTS: call .step off
class A1 {
  k: number = 3;
  step(x: number): number { return x * this.k + 7; }
}
class B1 extends A1 {
  step(x: number): number { return x * this.k + 11; }
}
const cs: A1[] = [new A1(), new B1()];
let s = 0;
for (let i = 0; i < 4; i++) s += cs[i & 1].step(i);
console.log(s);
