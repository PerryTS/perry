// Each evaluation of a capturing class expression is its own constructor with its
// own captures; instances, instanceof and method values follow their evaluation.
function make(tag: string) {
  return class {
    inst = tag;
    who() {
      return tag;
    }
  };
}
const A = make("A");
const B = make("B");
const a: any = new A();
const b: any = new B();
console.log(a.who(), b.who(), a.inst, b.inst, A === B);
console.log(a instanceof A, a instanceof B, b instanceof B);
console.log(a.who.call(b), A.prototype.who.call(b), Reflect.apply(A.prototype.who, b, []));
const borrowed = { who: b.who, inst: "x" };
console.log(borrowed.who());

function counter(start: number) {
  let n = start;
  const K = class {
    bump() {
      n = n + 1;
      return n;
    }
    peek() {
      return n;
    }
  };
  n = start * 10;
  return K;
}
const C1 = counter(1);
const C2 = counter(2);
const c1 = new C1();
console.log(c1.peek(), new C2().peek());
console.log(c1.bump(), new C1().peek(), new C2().bump());

function declared(t: string) {
  class K {
    v: number;
    constructor(v: number) {
      this.v = v;
    }
    who() {
      return t + this.v;
    }
    again() {
      return new K(this.v + 1);
    }
  }
  return new K(1);
}
const x = declared("x");
const y = declared("y");
console.log(x.who(), y.who(), x.again().who(), y.again().who());
function bare() {
  return class {};
}
console.log(bare() === bare());
