// `x instanceof C` for a class C whose constructor chain is other classes
// (a DOM-style EventTarget <- Node <- Element <- HTMLElement <- C chain).
// A class hop with no @@hasInstance in its own keys, its static-symbol table
// or a lifted `static [Symbol.hasInstance]` is answered by the prototype walk;
// every way a hop can supply a hook still runs it, with C as its receiver,
// once per test, including hooks installed after the site ran.

class EventTargetX {}
class NodeX extends EventTargetX {}
class ElementX extends NodeX {}
class HTMLElementX extends ElementX {}
class ImageX extends HTMLElementX {}

const img = new ImageX();
const plain: any = { data: null, width: 2, height: 2 };
const bare: any = Object.create(null);

function test(v: any, C: any): string {
  try {
    return String(v instanceof C);
  } catch (e: any) {
    return e.constructor.name;
  }
}

function round(tag: string): void {
  console.log(
    tag,
    test(img, ImageX),
    test(img, ElementX),
    test(plain, ImageX),
    test(bare, ImageX),
    test(1, ImageX),
    test("s", ImageX),
    test(null, ImageX),
  );
}

round("plain chain");

// The same site, many times (a hot site sees the ordinary walk).
let hits = 0;
for (let i = 0; i < 1000; i++) {
  if ((i & 1 ? img : plain) instanceof ImageX) hits++;
}
console.log("loop", hits);

// A prototype replaced on an intermediate hop is followed exactly.
const elementProto = ElementX.prototype;
Object.setPrototypeOf(HTMLElementX.prototype, Object.prototype);
round("relinked prototype");
Object.setPrototypeOf(HTMLElementX.prototype, elementProto);
round("restored prototype");

// A hook defined on a parent class after the site ran: inherited by ImageX.
let calls = 0;
Object.defineProperty(NodeX, Symbol.hasInstance, {
  value(this: any, v: any) {
    calls++;
    return this === ImageX && v === plain;
  },
  configurable: true,
});
round("parent defineProperty hook");
console.log("calls", calls);
delete (NodeX as any)[Symbol.hasInstance];
round("hook deleted");

// An accessor hook on a parent hop: the getter runs once per test.
let gets = 0;
Object.defineProperty(ElementX, Symbol.hasInstance, {
  get() {
    gets++;
    return (v: any) => v === bare;
  },
  configurable: true,
});
round("parent getter hook");
console.log("gets", gets);
delete (ElementX as any)[Symbol.hasInstance];

// A getter that returns undefined falls through to the ordinary walk.
Object.defineProperty(ElementX, Symbol.hasInstance, {
  get() {
    gets++;
    return undefined;
  },
  configurable: true,
});
round("undefined getter");
console.log("gets", gets);
delete (ElementX as any)[Symbol.hasInstance];

// A non-callable hook throws a TypeError.
Object.defineProperty(HTMLElementX, Symbol.hasInstance, { value: 42, configurable: true });
round("non-callable hook");
delete (HTMLElementX as any)[Symbol.hasInstance];

// A lifted static hook on a parent class, inherited through the chain.
class HookBase {
  static [Symbol.hasInstance](v: any): boolean {
    return v === plain;
  }
}
class HookMid extends HookBase {}
class HookLeaf extends HookMid {}
console.log("static hook", test(plain, HookLeaf), test(new HookLeaf(), HookLeaf), test(img, HookMid));

// A static symbol field (not a method) that is not @@hasInstance changes nothing.
const tag = Symbol("tag");
class Tagged extends HTMLElementX {
  static [tag] = 1;
}
console.log("static symbol field", test(new Tagged(), Tagged), test(img, Tagged), test(new Tagged(), ElementX));

// The constructor's [[Prototype]] replaced after the site ran: a hook on the
// new parent object applies.
class Swapped extends ElementX {}
const sw = new Swapped();
console.log("before swap", test(sw, Swapped), test(plain, Swapped));
Object.setPrototypeOf(Swapped, {
  [Symbol.hasInstance](v: any) {
    return v === plain;
  },
});
console.log("after swap", test(sw, Swapped), test(plain, Swapped));

// A Proxy on the constructor chain sees the @@hasInstance read.
class ProxLeaf extends NodeX {}
const seen: string[] = [];
Object.setPrototypeOf(
  ProxLeaf,
  new Proxy(NodeX, {
    get(t, k, r) {
      seen.push(typeof k === "symbol" ? k.toString() : String(k));
      return Reflect.get(t, k, r);
    },
  }),
);
console.log("proxy chain", test(new ProxLeaf(), ProxLeaf), test(plain, ProxLeaf), seen.join(","));

// Bound functions as the right-hand side.
const BoundImage = (ImageX as any).bind(null);
console.log("bound", test(img, BoundImage), test(plain, BoundImage));

// A plain function whose prototype property is replaced, and one whose
// prototype is a getter-free non-object.
function Fn(this: any) {}
const fn: any = new (Fn as any)();
console.log("function", test(fn, Fn));
(Fn as any).prototype = ImageX.prototype;
console.log("function reassigned", test(fn, Fn), test(img, Fn));
(Fn as any).prototype = 5;
console.log("function primitive prototype", test(fn, Fn), test(1, Fn));
