// #12668 (macOS): the runtime finds static atoms and layout descriptors by
// walking their sections, and many have no reference from code. Mach-O's
// `-dead_strip` dropped them, so the program's key atoms were never seeded:
// a class method `foo` plus an object literal key `foo` gave the literal's
// key list a second (heap) atom, and a coloured read of `p.foo` through the
// image atom answered absent. zod's `this.refinement.bind(this)` then threw
// "Bind must be called on a function".

class A {
  foo() {
    return 1;
  }
  refinement(check: (v: number) => boolean) {
    return this._refinement((v: number) => check(v));
  }
  _refinement(refinement: (v: number) => boolean) {
    return { type: "refinement", refinement };
  }
}
const lit = { bar: 2, foo: 3 };

function kind(p: any): string {
  return typeof p.foo;
}
function value(p: any): any {
  return p.foo;
}
console.log("proto", kind(A.prototype), kind(new A()));
console.log("literal", kind(lit), value(lit));
console.log("refinement", typeof (A.prototype as any).refinement);

class Base {
  constructor() {
    (this as any).refinement = (this as any).refinement.bind(this);
  }
  refinement() {
    return "bound";
  }
}
class Sub extends Base {}
function make(): any {
  return new Sub();
}
console.log("bind", make().refinement());

const shorthand = (() => {
  const refinement = 5;
  return { refinement };
})();
function readRefinement(p: any): any {
  return p.refinement;
}
console.log("shorthand", readRefinement(shorthand));
