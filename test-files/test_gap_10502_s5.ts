// Refs #10502: a receiver word survives mutations of its holder.
class Root { m() { return 1; } }
class Base extends Root { m() { return 2; } value = 7; }
class A extends Base { a = 1; }
class B extends Base { b = 2; }
class C extends Base { c = 3; }
class D extends Base { d = 4; }
class E extends Base { e = 5; }
class F extends Base { f = 6; }
class G extends Base { g = 7; }
class H extends Base { h = 8; }
class I extends Base { i = 9; }
class J extends Base { j = 10; }
const monoObject = new Base();
const polyObjects = [new A(), new B()];
const megaObjects = [new A(), new B(), new C(), new D(), new E(), new F(), new G(), new H(), new I(), new J()];
function mono(o: Base) { return o.m(); }
function poly(o: any) { return o.m(); }
function mega(o: any) { return o.m(); }
function totals() {
  let p = 0, m = 0;
  for (const o of polyObjects) p += poly(o);
  for (const o of megaObjects) m += mega(o);
  return [mono(monoObject), p, m].join(",");
}
for (let n = 0; n < 40; n++) totals();
console.log("warm", totals());
const original = Base.prototype.m;
Base.prototype.m = function () { return 3; };
console.log("patch", totals(), original.call(monoObject));
let getters = 0;
Object.defineProperty(Base.prototype, "m", {
  configurable: true,
  get() { getters++; return function () { return 4; }; }
});
console.log("accessor", totals(), getters);
delete (Base.prototype as any).m;
console.log("delete", totals());
const alternate = { m() { return 5; } };
Object.setPrototypeOf(Base.prototype, alternate);
console.log("holder relink", totals());
Object.setPrototypeOf(monoObject, { m() { return 6; } });
Object.setPrototypeOf(polyObjects[0], { m() { return 8; } });
Object.setPrototypeOf(megaObjects[0], { m() { return 9; } });
console.log("receiver relink", totals());

// Warm an inherited site, then insert a nearer shadow without touching the
// original terminal holder. Every intermediate prototype shape matters.
class Ancestor { inherited() { return 10; } }
class Middle extends Ancestor {}
class Leaf extends Middle {}
const leaf = new Leaf();
function inherited(o: Leaf) { return o.inherited(); }
for (let n = 0; n < 40; n++) inherited(leaf);
console.log("inherited warm", inherited(leaf));
(Middle.prototype as any).inherited = function () { return 11; };
console.log("intermediate patch", inherited(leaf));
Object.defineProperty(Middle.prototype, "inherited", { configurable: true, get() { return function () { return 12; }; } });
console.log("intermediate accessor", inherited(leaf));
delete (Middle.prototype as any).inherited;
Object.setPrototypeOf(Middle.prototype, { inherited() { return 13; } });
console.log("intermediate relink", inherited(leaf));

class Parent { x: number = 20; }
class Child extends Parent { y: number = 1; }
class Other { x: number = 30; }
const child = new Child();
function ancestry(o: any) { return [o instanceof Child, o instanceof Parent, o instanceof Other].join(","); }
for (let n = 0; n < 40; n++) ancestry(child);
console.log("ancestry warm", ancestry(child));
Object.setPrototypeOf(Child, Other);
console.log("constructor ancestry", ancestry(child));
Object.setPrototypeOf(Child.prototype, Other.prototype);
console.log("class ancestry", ancestry(child));
Object.setPrototypeOf(child, Parent.prototype);
console.log("instance ancestry", ancestry(child));
Object.setPrototypeOf(child, null);
console.log("null ancestry", ancestry(child), child instanceof Object);
Object.setPrototypeOf(child, []);
console.log("array ancestry", ancestry(child), child instanceof Object, child instanceof Array);
console.log("boxed ancestry", new Number(3) instanceof Number, new String("x") instanceof String, new Boolean(false) instanceof Boolean);

class HasInstance {
  static [Symbol.hasInstance](o: any) { return o && o.accept === 42; }
}
console.log("hasInstance", { accept: 42 } instanceof HasInstance, new HasInstance() instanceof HasInstance);
class Custom {}
Object.defineProperty(Custom, Symbol.hasInstance, { value: (o: any) => !!o && o.accept === 43, configurable: true });
console.log("installed hasInstance", { accept: 43 } instanceof Custom, new Custom() instanceof Custom);

// A subclass argument must preserve the base parameter's field accesses.
class ParamBase { x: number = 40; }
class ParamChild extends ParamBase { y: number = 2; }
const param = new ParamChild();
Object.getPrototypeOf(param);
function useParam(o: ParamBase) { return o.x + 1; }
for (let n = 0; n < 40; n++) useParam(param);
console.log("subclass param", useParam(param));
Object.setPrototypeOf(ParamChild.prototype, null);
console.log("relinked param", useParam(param), param instanceof ParamBase);
