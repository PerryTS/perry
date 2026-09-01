// #9369: a class carrying a generic computed member must not lose `this` in
// its STATIC bodies. `this` there is the class CONSTRUCTOR — an INT32 class
// ref — not an instance, so codegen may not route the read through the
// instance-shaped by-name helper that strips the NaN-box to an
// `ObjectHeader*`. Doing so handed the runtime the bare class id as a
// pointer, and every static-`this` read but `name` answered `undefined`
// while the same property read through the class's own binding answered an
// object — one function, two answers.
//
// The member kinds below are the ones that reach the generic computed path:
// `[Symbol.iterator]` (generator and non-generator), `[Symbol.asyncIterator]`,
// `[Symbol.toPrimitive]`, and a plain computed key. #9315 routed the
// well-known-symbol forms onto it, which is how axios's `AxiosHeaders`
// (`[Symbol.iterator]()` plus `static accessor(){ let z = this.prototype; … }`)
// took `cc --help` down with `Object.defineProperty called on non-object`
// (#9341).

function T(name, fn) {
  try { const r = fn(); console.log(name + " => " + String(r)); }
  catch (e) { console.log(name + " !! " + (e && e.message ? e.message : String(e))); }
}
class A {
  static before() { return typeof this.prototype; }
  toJSON() { return { a: 1 }; }
  [Symbol.iterator]() { return Object.entries(this.toJSON())[Symbol.iterator](); }
  static after() { return typeof this.prototype; }
  method() { return typeof this; }
}
T("static-BEFORE-computed", () => A.before());
T("static-AFTER-computed", () => A.after());
T("instance-method-after", () => new A().method());
T("named-binding", () => typeof A.prototype);

// which computed keys trigger it?
class B { toJSON(){return{a:1}} [Symbol.asyncIterator]() { return null; } static after() { return typeof this.prototype; } }
T("asyncIterator-trigger", () => B.after());
class C { toJSON(){return{a:1}} *[Symbol.iterator]() { yield 1; } static after() { return typeof this.prototype; } }
T("generator-iterator-trigger", () => C.after());
class D { toJSON(){return{a:1}} [Symbol.toPrimitive]() { return 1; } static after() { return typeof this.prototype; } }
T("toPrimitive-trigger", () => D.after());
const K = "dyn" + "Key";
class E { toJSON(){return{a:1}} [K]() { return 1; } static after() { return typeof this.prototype; } }
T("plain-computed-key-trigger", () => E.after());
