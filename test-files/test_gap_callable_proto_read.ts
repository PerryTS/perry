// Reads on a callable class instance (a bound function whose [[Prototype]] is
// a class prototype): inherited data, getters with the callable as receiver,
// shadowing, and prototype edits after construction.
class Callable {
  constructor(fn: (...a: any[]) => any, attach?: object) {
    return Object.assign(Object.setPrototypeOf(fn.bind(this), new.target.prototype), attach);
  }
}
class Base extends Callable {
  tag = "base-field";
  constructor(attach?: object) {
    super((x: number) => x * 2, attach);
  }
  method() {
    return "Base.method";
  }
  get self() {
    return this;
  }
  get kindName() {
    return "kind:" + (this as any).kind;
  }
}
class Mid extends Base {
  midMethod() {
    return "Mid.midMethod";
  }
}
class Leaf extends Mid {
  method() {
    return "Leaf.method/" + super.method();
  }
}
const n = new Leaf({ kind: "leaf" }) as any;
console.log(typeof n, n(21));
console.log(n.method(), n.midMethod(), typeof n.method, n.method === Leaf.prototype.method);
console.log(n.self === n, n.kindName, n.tag, n.kind);
console.log(n.missing, "missing" in n, typeof n.toString, typeof n.hasOwnProperty);
console.log(n.call === Function.prototype.call, typeof n.bind);
// data added to a prototype after construction
(Mid.prototype as any).later = 42;
console.log(n.later);
(Mid.prototype as any).later = undefined;
console.log(n.later);
(Mid.prototype as any).later = null;
console.log(n.later);
// an accessor installed over an inherited data property
Object.defineProperty(Base.prototype, "method", { get() { return () => "accessor:" + (this === n); }, configurable: true });
console.log(n.midMethod(), n.method());
delete (Leaf.prototype as any).method;
console.log(n.method());
// own property shadows
n.midMethod = () => "own";
console.log(n.midMethod());
// relink the callable to another prototype
Object.setPrototypeOf(n, { other: "other-proto", method() { return "relinked"; } });
console.log(n.other, n.method(), n.midMethod(), n.tag);
// a getter on the chain returning undefined, then a data value behind it
const p2 = Object.create({ deep: "deep-data" });
Object.defineProperty(p2, "g", { get() { return undefined; } });
const m = Object.setPrototypeOf(function () {}, p2) as any;
console.log(m.g, m.deep);
// GPP2: the inherited read is the prototype's OrdinaryGet with the callable
// as receiver, in one walk.
class Getters extends Callable {
  constructor(attach?: object) {
    super(() => 0, attach);
  }
  get nul() {
    return null;
  }
  get und() {
    return undefined;
  }
  set onlySet(v: any) {}
  get viaThis() {
    return typeof this === "function" ? "callable:" + (this as any).id : "other";
  }
}
class G2 extends Getters {
  value = 7;
}
class G3 extends G2 {}
const g = new G3({ id: "g3" }) as any;
console.log(g.nul, g.und, g.onlySet, g.viaThis, g.value);
console.log(g.constructor === G3, g.constructor.name, Object.getPrototypeOf(g) === G3.prototype);
console.log(Object.getPrototypeOf(G3.prototype) === G2.prototype, Object.getPrototypeOf(G2.prototype) === Getters.prototype);
// data null / undefined deep on the chain
(Getters.prototype as any).deepNull = null;
(Getters.prototype as any).deepUndef = undefined;
console.log(g.deepNull, g.deepUndef, "deepNull" in g);
// %Function.prototype% is not on a class instance's chain
(Function.prototype as any).fpExpando = "fp";
console.log(g.fpExpando, (function () {} as any).fpExpando);
delete (Function.prototype as any).fpExpando;
// symbol keys and later prototype edits
const sym = Symbol("s");
(G2.prototype as any)[sym] = "sym-value";
console.log(g[sym]);
Object.defineProperty(G2.prototype, "late", { get() { return this === g ? "late:callable" : "late:other"; }, configurable: true });
console.log(g.late);
Object.defineProperty(G2.prototype, "late", { value: "late:data", configurable: true });
console.log(g.late);
// the same read many times keeps the answer (and a prototype edit in between)
let acc = "";
for (let i = 0; i < 3; i++) {
  acc += g.viaThis + "|";
  if (i === 1) Object.defineProperty(Getters.prototype, "viaThis", { get() { return "redef"; }, configurable: true });
}
console.log(acc);
// a declared class instance reads its [[Prototype]] from its shape
class PA { a = 1; m() { return 1; } }
class PB extends PA { b = 2; }
const pb: any = new PB();
const gp = Object.getPrototypeOf;
console.log("1", Object.getPrototypeOf(pb) === PB.prototype);
console.log("2", gp(pb) === PB.prototype, gp(PB.prototype) === PA.prototype, gp(PA.prototype) === Object.prototype);
console.log("3", Reflect.getPrototypeOf(pb) === PB.prototype, PB.prototype.isPrototypeOf(pb), PA.prototype.isPrototypeOf(PB.prototype));
const chain: string[] = []; for (let p = gp(pb); p; p = gp(p)) chain.push(p === PB.prototype ? "PB" : p === PA.prototype ? "PA" : p === Object.prototype ? "O" : "?");
console.log("4", chain.join(">"));
console.log("5", (pb as any).__proto__ === PB.prototype);
