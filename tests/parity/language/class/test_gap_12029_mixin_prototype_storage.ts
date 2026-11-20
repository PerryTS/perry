// #12029: the class id names a template, never another evaluation's properties.
class Base { who() { return "base"; } }
const ReproMixin = (S: any, tag: string) => class extends S {
  who() { return tag + ":" + super.who(); }
};
const ReproA = ReproMixin(Base, "a");
const ReproB = ReproMixin(Base, "b");
console.log(ReproA.prototype === ReproB.prototype);
console.log(new ReproA().who(), new ReproB().who());
ReproA.prototype.extra = 1;
console.log("repro", new ReproB().extra);

// Captured heritage and class declarations take the same ownership rule.
function declaredMixin(S: any, tag: string) {
  class Declared extends S { who() { return tag + ":" + super.who(); } }
  return Declared;
}
const DeclaredA = declaredMixin(Base, "da");
const DeclaredB = declaredMixin(Base, "db");
DeclaredA.prototype.extra = 8;
console.log("shared-first", new DeclaredA().extra, new DeclaredB().extra, DeclaredB.prototype.extra);

// #11759's shared first evaluation must also own its storage alone.
function sharedFirst() {
  class Plain { who() { return "plain"; } }
  return Plain;
}
const PlainA = sharedFirst();
const PlainB = sharedFirst();
console.log("plain-identities", PlainA === PlainB, PlainA.prototype === PlainB.prototype);
(PlainA.prototype as any).extra = 9;
console.log("plain-storage", (new PlainA() as any).extra, (new PlainB() as any).extra, (PlainB.prototype as any).extra);
(PlainB.prototype as any).extra = 10;
delete (PlainB.prototype as any).extra;
console.log("plain-delete", (new PlainA() as any).extra, (new PlainB() as any).extra);
Object.defineProperty(PlainB.prototype, "who", {
  value: function () { return "plain-b-defined"; }, configurable: true,
});
console.log("plain-define", new PlainA().who(), new PlainB().who());
delete (PlainB.prototype as any).who;
console.log("plain-delete-method", new PlainA().who(), typeof (new PlainB() as any).who);
console.log("plain-keys", Object.keys(PlainA.prototype).join(","), Object.keys(PlainB.prototype).join(","));
// Mutating the first evaluation before the next one must not change ClassBody.
function beforeNext() {
  class Before {
    who() { return "before-body"; }
    static label() { return "before-static"; }
  }
  return Before;
}
const BeforeA = beforeNext();
(BeforeA.prototype as any).extra = 11;
delete (BeforeA.prototype as any).who;
delete (BeforeA as any).label;
const BeforeB = beforeNext();
console.log("before-next", typeof (new BeforeA() as any).who, new BeforeB().who(), (new BeforeA() as any).extra, (new BeforeB() as any).extra, typeof (BeforeA as any).label, BeforeB.label());
console.log("before-keys", Object.getOwnPropertyNames(BeforeA.prototype).join(","), Object.getOwnPropertyNames(BeforeB.prototype).join(","));

const Mixin = (S: any, tag: string) => class extends S {
  who() { return tag + ":" + super.who(); }
  static tag = tag;
  static label() { return tag; }
};
const A = Mixin(Base, "a");
const B = Mixin(Base, "b");
const a = new A();
const b = new B();
console.log(A.prototype === B.prototype, a.who(), b.who());
A.prototype.extra = 1;
console.log("add-a", a.extra, b.extra, new B().extra, B.prototype.extra);
B.prototype.extra = 2;
B.prototype.onlyB = 3;
console.log("add-b", a.extra, b.extra, a.onlyB, b.onlyB);
console.log("keys", Object.keys(A.prototype).join(","), Object.keys(B.prototype).join(","));
delete B.prototype.extra;
console.log("delete-b", a.extra, b.extra, B.prototype.extra);
Object.defineProperty(B.prototype, "value", {
  get() { return this.saved === undefined ? "b-default" : this.saved; },
  set(v) { this.saved = v; }, enumerable: true, configurable: true,
});
console.log("accessor", a.value, b.value, A.prototype.value);
b.value = "b-set";
console.log("set", a.value, b.value, b.saved, a.saved);
Object.defineProperty(A.prototype, "value", {
  get() { return "a-get"; }, enumerable: true, configurable: true,
});
delete B.prototype.value;
console.log("delete-accessor", a.value, b.value);
Object.defineProperty(B.prototype, "onlyB", { value: 4, enumerable: true, configurable: true });
Object.defineProperty(B.prototype, "who", {
  value: function () { return "b-replaced"; }, configurable: true,
});
console.log("define", a.onlyB, b.onlyB, a.who(), b.who());
delete B.prototype.who;
console.log("delete-method", a.who(), b.who());
console.log("keys-after", Object.keys(A.prototype).join(","), Object.keys(B.prototype).join(","));
// A later evaluation starts from the ClassBody even after earlier mutations.
const C = Mixin(Base, "c");
console.log("later", new C().extra, new C().onlyB, new C().value, new C().who(), Object.keys(C.prototype).length);
A.tag = "a-static";
B.staticExtra = 5;
console.log("static", A.tag, B.tag, C.tag, A.label(), B.label(), C.label(), A.staticExtra, B.staticExtra, C.staticExtra);
// One template, three levels, and two disjoint evaluation chains.
const A2 = Mixin(A, "a2");
const A3 = Mixin(A2, "a3");
const B2 = Mixin(B, "b2");
const B3 = Mixin(B2, "b3");
A2.prototype.middle = "a-middle";
B3.prototype.leaf = "b-leaf";
const aa = new A3();
const bb = new B3();
console.log("chain", aa.who(), bb.who(), aa.extra, bb.extra, aa.middle, bb.middle, aa.leaf, bb.leaf);
console.log("instanceof", aa instanceof A3, aa instanceof A2, aa instanceof A, aa instanceof Base, aa instanceof B, bb instanceof B3, bb instanceof B2, bb instanceof B, bb instanceof A);
console.log("links", Object.getPrototypeOf(aa) === A3.prototype, Object.getPrototypeOf(A3.prototype) === A2.prototype, Object.getPrototypeOf(B3.prototype) === B2.prototype);
// No method entries: ownership must still be per evaluation.
const Empty = () => class {};
const E = Empty();
const F = Empty();
E.prototype.x = 6;
Object.defineProperty(F.prototype, "x", { value: 7, configurable: true, enumerable: true });
delete F.prototype.x;
console.log("empty", new E().x, new F().x, Object.keys(E.prototype).join(","), Object.keys(F.prototype).join(","));
