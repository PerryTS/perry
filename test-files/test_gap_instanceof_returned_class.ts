// Returned class values must keep the same instance prototype identity when
// used as superclass, constructor, or instanceof RHS across module imports.
import * as ns from "./fixtures/instanceof_returned_class/ns.ts";
import { Foo } from "./fixtures/instanceof_returned_class/ns.ts";
class A extends ns.Foo {}
class B extends Foo {}
class C extends ns.Bar {}
const F = ns.Foo;
console.log(new A(1) instanceof ns.Foo, new B(1) instanceof Foo,
  new C(1) instanceof ns.Bar, new F(1) instanceof ns.Foo,
  new A(1) instanceof F, new A(1) instanceof A);
console.log("edges", Object.getPrototypeOf(A) === ns.Foo,
  Object.getPrototypeOf(A.prototype) === ns.Foo.prototype,
  Object.getPrototypeOf(B.prototype) === Foo.prototype);
class D extends ns.Decl {}
console.log("declaration", new D(2) instanceof ns.Decl, new D(2) instanceof D,
  Object.getPrototypeOf(D.prototype) === ns.Decl.prototype);

const E1 = ns.expressionFactory();
const E2 = ns.expressionFactory();
class ES1 extends E1 {}
class ES2 extends E2 {}
console.log("expressions", E1 === E2, new E1(3) instanceof E1,
  new E1(3) instanceof E2, new E2(4) instanceof E1, new E2(4) instanceof E2);
console.log("expression subclasses", new ES1(5) instanceof E1,
  new ES1(5) instanceof E2, new ES2(6) instanceof E1, new ES2(6) instanceof E2);

const D1 = ns.declarationFactory();
const D2 = ns.declarationFactory();
class DS1 extends D1 {}
class DS2 extends D2 {}
console.log("declarations", D1 === D2, new D1(7) instanceof D1,
  new D1(7) instanceof D2, new D2(8) instanceof D1, new D2(8) instanceof D2);
console.log("declaration subclasses", new DS1(9) instanceof D1,
  new DS1(9) instanceof D2, new DS2(10) instanceof D1, new DS2(10) instanceof D2);

class Err extends ns.WithHeritage {}
const error = new Err("message");
console.log("heritage", error instanceof Err, error instanceof ns.WithHeritage,
  error instanceof Error, Object.getPrototypeOf(Err.prototype) === ns.WithHeritage.prototype);
