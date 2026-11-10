// Construction must use the exact evaluation, including after a binding changes.
function factory(tag: string) {
  return class {
    tag;
    target;
    proto;
    constructor(value: number) {
      this.tag = tag + value;
      this.target = new.target;
      this.proto = Object.getPrototypeOf(this);
    }
  };
}
const A = factory("a");
const B = factory("b");
function construct(C: any, value: number) { return new C(value); }
const a = construct(A, 1);
const b = construct(B, 2);
console.log("evaluations", A === B, A.prototype === B.prototype, a.tag, b.tag);
console.log("targets", a.target === A, b.target === B, a instanceof A, a instanceof B);
let Current: any = A;
console.log("before", construct(Current, 3).tag);
Current = B;
console.log("after", construct(Current, 4).tag);
let evaluations = 0;
let Parent: any = A;
function parent() { evaluations++; return Parent; }
const Derived = (() => class extends parent() {
  extra = "derived";
  constructor(value: number) { super(value); }
})();
Parent = B;
const d1 = construct(Derived, 5);
const d2 = construct(Derived, 6);
console.log("heritage", evaluations, d1.tag, d2.tag, d1.extra, d1.target === Derived);
console.log("chain", d1 instanceof A, d1 instanceof B, d1 instanceof Derived);
function derive(base: any) {
  return class extends base {
    constructor(value: number) { super(value); }
  };
}
const DerivedA = derive(A);
const DerivedB = derive(B);
console.log("parent evaluations", construct(DerivedA, 19).tag, construct(DerivedB, 20).tag,
  construct(DerivedA, 21).tag);
const NullBase: any = class extends null {
  constructor() { return Object.create(null); }
};
console.log("null return", Object.getPrototypeOf(construct(NullBase, 0)) === null);
const BadNull: any = class extends null {};
try { construct(BadNull, 0); } catch (error) { console.log("null super", error instanceof TypeError); }
const replacement = { result: "replacement" };
function returning() { return class { constructor() { return replacement; } }; }
const Returns: any = returning();
console.log("return", construct(Returns, 0) === replacement);
const reflected: any = Reflect.construct(A, [7], B);
console.log("reflect", reflected.tag, reflected.target === B,
  Object.getPrototypeOf(reflected) === B.prototype, reflected instanceof B, reflected.proto === B.prototype);
console.log("reflect return", Reflect.construct(Returns, [], B) === replacement,
  Object.getPrototypeOf(replacement) === Object.prototype);
const Defaulted: any = class { value; constructor(value = 12) { this.value = value; } };
console.log("default", construct(Defaulted, undefined).value);
const Packed: any = class { value; constructor(first: number, ...rest: number[]) { this.value = [first, rest.length, arguments.length].join(":"); } };
function packed(C: any) { return new C(8, 9, 10); }
console.log("packing", packed(Packed).value);
const ArgumentsOnly: any = (() => class { value; constructor(first: number) { this.value = [first, arguments.length].join(":"); } })();
console.log("arguments", packed(ArgumentsOnly).value);
function capless() {
  return class {
    #value;
    target;
    constructor(value: number) { this.#value = value; this.target = new.target; }
    read() { return this.#value; }
  };
}
const CaplessA = capless();
const CaplessB = capless();
const ca = construct(CaplessA, 16);
const cb = construct(CaplessB, 17);
console.log("capless", ca.read(), cb.read(), ca.target === CaplessA, cb.target === CaplessB);
try { CaplessA.prototype.read.call(cb); } catch (error) { console.log("capless brand", error instanceof TypeError); }
class Declared { value; constructor(value = 14) { this.value = value; } }
console.log("declared", construct(Declared, undefined).value);
function Plain(value: number) { this.value = "plain" + value; }
console.log("function", construct(Plain, 15).value);
