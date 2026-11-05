// #11043: a capture-carrying class (one whose members close over a local of
// the enclosing function) gets a distinct prototype object per evaluation.
// Its ClassBody accessors must be own properties of that prototype, exactly
// like a top-level class's. whatwg-url's generated `URL` wrapper is the shape
// that broke mongodb: the class is declared inside `install(globalObject)`,
// its accessors close over `globalObject`, and the module then runs
// `Object.defineProperties(URL.prototype, { pathname: { enumerable: true } })`.
// A generic descriptor must only flip `enumerable` — before the fix it
// replaced the accessor with a read-only `undefined` data property and
// `url.pathname = "/"` threw "Cannot assign to read only property".

function describe(label: string, proto: object, key: string): void {
  const d = Object.getOwnPropertyDescriptor(proto, key);
  if (d === undefined) {
    console.log(label, key, "missing");
    return;
  }
  console.log(
    label,
    key,
    "get:" + typeof d.get,
    "set:" + typeof d.set,
    "enumerable:" + d.enumerable,
    "configurable:" + d.configurable,
    "value" in d ? "has-value" : "no-value",
  );
}

// 1. The whatwg-url shape.
const implSymbol = Symbol("impl");
function install(globalObject: any): void {
  class URL {
    constructor(href: string) {
      const wrapper = Object.create(new.target.prototype);
      Object.defineProperty(wrapper, implSymbol, {
        value: { href, pathname: "" },
        configurable: true,
      });
      return wrapper;
    }
    get href(): string {
      const esValue = this !== null && this !== undefined ? this : globalObject;
      return esValue[implSymbol].href;
    }
    get pathname(): string {
      const esValue = this !== null && this !== undefined ? this : globalObject;
      return esValue[implSymbol].pathname;
    }
    set pathname(v: string) {
      const esValue = this !== null && this !== undefined ? this : globalObject;
      esValue[implSymbol].pathname = String(v);
    }
    toJSON(): string {
      return (this as any)[implSymbol].href;
    }
  }
  console.log("own before", Object.getOwnPropertyNames(URL.prototype).join(","));
  describe("before", URL.prototype, "pathname");
  Object.defineProperties(URL.prototype, {
    toJSON: { enumerable: true },
    href: { enumerable: true },
    pathname: { enumerable: true },
    [Symbol.toStringTag]: { value: "URL", configurable: true },
  });
  describe("after", URL.prototype, "pathname");
  describe("after", URL.prototype, "href");
  describe("after", URL.prototype, "toJSON");
  console.log("keys after", Object.keys(URL.prototype).join(","));
  Object.defineProperty(globalObject, "URL", { configurable: true, writable: true, value: URL });
}
const sharedGlobalObject: any = {};
install(sharedGlobalObject);
const WURL = sharedGlobalObject.URL;

const u = new WURL("mongodb://h/x");
u.pathname = "/db";
console.log("set on instance", u.pathname, Object.prototype.toString.call(u));

// 2. Reflection on a plain capturing accessor, per evaluation.
function makeCounter(start: number) {
  let n = start;
  class Counter {
    get value(): number {
      return n;
    }
    set value(v: number) {
      n = v;
    }
    get readOnly(): number {
      return n * 2;
    }
    bump(): number {
      return ++n;
    }
  }
  return Counter;
}
const A = makeCounter(1);
const B = makeCounter(10);
console.log("distinct prototypes", A.prototype !== B.prototype);
describe("A", A.prototype, "value");
describe("A", A.prototype, "readOnly");
describe("A", A.prototype, "bump");
console.log(
  "hasOwn",
  Object.prototype.hasOwnProperty.call(A.prototype, "value"),
  Object.prototype.hasOwnProperty.call(B.prototype, "readOnly"),
  Object.prototype.hasOwnProperty.call(new A(), "value"),
);
console.log("names A", Object.getOwnPropertyNames(A.prototype).join(","));
console.log("names B", Object.getOwnPropertyNames(B.prototype).join(","));
Object.defineProperty(B.prototype, "value", { enumerable: true });
describe("B redefined", B.prototype, "value");
const b = new B();
b.value = 42;
console.log("B setter after redefine", b.value, b.bump(), b.readOnly);
const a = new A();
a.value = 7;
console.log("A untouched", a.value, a.bump());

// A reflected getter still reads through the receiver.
const getter = Object.getOwnPropertyDescriptor(A.prototype, "readOnly")!.get!;
console.log("reflected getter", getter.call(a));

// 3. Getter-only accessor stays getter-only after a generic redefine.
function makeFrozen(tag: string) {
  class Frozen {
    get tag(): string {
      return tag;
    }
  }
  Object.defineProperties(Frozen.prototype, { tag: { enumerable: true } });
  return Frozen;
}
const F = makeFrozen("t1");
describe("F", F.prototype, "tag");
const f: any = new F();
console.log("F reads", f.tag, Object.keys(F.prototype).join(","));
