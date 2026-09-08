// #9502: heritage alone must give a function-body declaration a fresh class.
function mk(P: any): any {
  class D extends (P ?? Object) {}
  return D;
}
const A = mk(null);
const B = mk(A);
const C = mk(B);
console.log("ident " + (A === B) + " " + (B === C));
console.log("gp " + (Object.getPrototypeOf(B) === A));
console.log("chain " + (Object.getPrototypeOf(C) === B));
console.log("prototypes " + (A.prototype !== B.prototype) + " " + (B.prototype !== C.prototype));
console.log("prototype chain " + (Object.getPrototypeOf(B.prototype) === A.prototype)
  + " " + (Object.getPrototypeOf(C.prototype) === B.prototype));
// Construct older evaluations after the template has seen newer parents.
const a = new A();
const b = new B();
const c = new C();
console.log("a " + (a instanceof A) + " " + (a instanceof B) + " " + (a instanceof C));
console.log("b " + (b instanceof A) + " " + (b instanceof B) + " " + (b instanceof C));
console.log("c " + (c instanceof A) + " " + (c instanceof B) + " " + (c instanceof C));

// Distinct roots must remain attached to their own escaped evaluations.
class Left { side() { return "left"; } }
class Right { side() { return "right"; } }
const L = mk(Left);
const R = mk(Right);
console.log("roots " + (Object.getPrototypeOf(L) === Left) + " " + (Object.getPrototypeOf(R) === Right));
console.log("methods " + new L().side() + " " + new R().side());
console.log("saved " + (Object.getPrototypeOf(C) === B) + " " + (new C() instanceof A));
