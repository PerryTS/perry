// #5592: new.target in class field initializers
// ES2025 §15.7.10: field inits run with [[Call]] so [[NewTarget]] === undefined.

// Direct new.target in a public field initializer
class C1 {
  x = new.target;
}
const c1 = new C1();
console.log(String(c1.x));  // undefined

// Direct new.target in an arrow inside a field initializer
class C2 {
  x = (() => new.target)();
}
const c2 = new C2();
console.log(String(c2.x));  // undefined

// eval("new.target") inside a public field initializer
class C3 {
  x = eval("new.target");
}
const c3 = new C3();
console.log(String(c3.x));  // undefined

// eval in arrow inside field init
class C4 {
  x = (() => eval("new.target"))();
}
const c4 = new C4();
console.log(String(c4.x));  // undefined

// eval in nested arrows inside field init
class C5 {
  x = (() => (() => eval("new.target"))())();
}
const c5 = new C5();
console.log(String(c5.x));  // undefined

// Private field initializer: same rule
class C6 {
  #x = new.target;
  getX() { return this.#x; }
}
const c6 = new C6();
console.log(String(c6.getX()));  // undefined

// Subclass: new.target in base class field init is still undefined
class Base {
  x = new.target;
}
class Derived extends Base {}
const d = new Derived();
console.log(String(d.x));  // undefined

// new.target in constructor is the actual constructor (unaffected)
class C7 {
  nt: unknown;
  constructor() {
    this.nt = new.target;
  }
}
console.log(new C7().nt === C7);  // true
