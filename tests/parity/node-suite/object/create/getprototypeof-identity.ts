// #3986: Object.getPrototypeOf must return the SAME object identity that was
// installed as the [[Prototype]] — for Object.create(proto) and for instances
// of a plain function constructor (whose [[Prototype]] is `F.prototype`).
// Previously these synthetic-class instances fell through to a self-prototype
// fallback (getPrototypeOf returned the instance itself), so the `=== proto`
// identity checks were false even though the prototype chain still worked.

// Object.create with an object-literal prototype.
const lit = { a: 1 };
const d = Object.create(lit);
console.log("create lit: proto identity", Object.getPrototypeOf(d) === lit);
console.log("create lit: isPrototypeOf", lit.isPrototypeOf(d));
console.log("create lit: inherited a", (d as any).a);

// Object.create with a function-constructed prototype (test262 15.2.3.5-3-1).
function Base(this: any) {}
const baseInst: any = new (Base as any)();
const created = Object.create(baseInst);
console.log("create fnobj: proto identity", Object.getPrototypeOf(created) === baseInst);
console.log("create fnobj: isPrototypeOf", baseInst.isPrototypeOf(created));

// Object.create with added own properties (test262 15.2.3.5-4-1).
const withProps = Object.create(baseInst, {
  x: { value: true, writable: false },
  y: { value: "str", writable: false },
});
console.log("create props: proto identity", Object.getPrototypeOf(withProps) === baseInst);
console.log("create props: x", (withProps as any).x, "y", (withProps as any).y);
console.log("create props: base untouched", (baseInst as any).x);

// Plain function constructor instance: [[Prototype]] is F.prototype.
function F(this: any, v: number) {
  this.value = v;
}
const inst: any = new (F as any)(7);
console.log("new F: proto is F.prototype", Object.getPrototypeOf(inst) === (F as any).prototype);
console.log("new F: proto.constructor", Object.getPrototypeOf(inst).constructor === F);

// Declared ES classes and object literals must be unaffected.
class C {}
const c = new C();
console.log("class: proto is C.prototype", Object.getPrototypeOf(c) === (C as any).prototype);
console.log("literal: proto is Object.prototype", Object.getPrototypeOf({}) === Object.prototype);
console.log("create(null): proto null", Object.getPrototypeOf(Object.create(null)) === null);
