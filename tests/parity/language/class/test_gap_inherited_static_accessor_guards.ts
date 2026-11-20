// Guards for the receiver of an inherited accessor: string-keyed static
// accessors, super.x in a static method, and an ordinary object whose
// [[Prototype]] is a function. Each already holds; they pin the rule beside
// the symbol-keyed fix.
class Own {
  static get x() {
    return this;
  }
  static set y(v: unknown) {
    (this as any).ys = v;
  }
  static who() {
    return this;
  }
}
class Deep extends Own {
  static m() {
    return super.x;
  }
  static w() {
    return super.who();
  }
  static sy(v: unknown) {
    super.y = v;
  }
}
class Deeper extends Deep {}
// 1. Inherited static getter.
console.log(Own.x === Own, Deep.x === Deep, Deeper.x === Deeper);
const k = "x";
console.log((Deep as any)[k] === Deep, Reflect.get(Own, "x", Deep) === Deep);
// 2. Inherited static setter writes on the receiver.
Deep.y = 7;
console.log(Object.prototype.hasOwnProperty.call(Deep, "ys"), Object.prototype.hasOwnProperty.call(Own, "ys"));
// 3. super.x / super.m() / super.y = v in static methods.
console.log(Deep.m() === Deep, Deeper.m() === Deeper, Deep.w() === Deep, Deeper.w() === Deeper);
Deeper.sy(3);
console.log(Object.prototype.hasOwnProperty.call(Deeper, "ys"), Object.prototype.hasOwnProperty.call(Deep, "ys"));
// 4. An ordinary object whose prototype is a function with an accessor.
function F() {}
Object.defineProperty(F, "x", { get() { return this; }, configurable: true });
Object.defineProperty(F, Symbol.species, { get() { return this; }, configurable: true });
const o: any = Object.create(F);
console.log(o.x === o, o[Symbol.species] === o);
const p: any = {};
Object.setPrototypeOf(p, F);
console.log(p.x === p);
class C {
  static get x() {
    return this;
  }
}
const q: any = Object.create(C);
console.log(q.x === q, q.x === C);
const base: any = { get g() { return this; } };
const child: any = Object.create(base);
console.log(child.g === child);
