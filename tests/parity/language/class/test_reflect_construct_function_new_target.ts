// `Reflect.construct(C, args, F)` with a class C and a newTarget F that is no class: the instance's [[Prototype]] is
// `F.prototype` (GetPrototypeFromConstructor), installed before C's constructor body runs; when `F.prototype` is no
// object the realm's `Object.prototype` is used. Before, the instance kept `C.prototype` (`b.read is not a
// function`). Controls: newTarget = C itself, a subclass of C, another class, `new C()`, a derived class target.
class C {
  x: number;
  seen: boolean;
  constructor(x: number) {
    this.x = x;
    this.seen = Object.getPrototypeOf(this) === C.prototype;
  }
  own(): string {
    return "C.own";
  }
}
class D extends C {
  own(): string {
    return "D.own";
  }
}
class Other {
  tag(): string {
    return "Other";
  }
}
function F(this: any) {}
F.prototype.read = function (this: any): string {
  return "F.read x=" + this.x;
};
function G() {}
(G as any).prototype = 5;

function describe(name: string, o: any): void {
  const p = Object.getPrototypeOf(o);
  const which =
    p === C.prototype ? "C" : p === D.prototype ? "D" : p === F.prototype ? "F" : p === Other.prototype ? "Other" : p === Object.prototype ? "Object" : "?";
  let r: string;
  try {
    r = typeof o.read === "function" ? o.read() : typeof o.own === "function" ? o.own() : "no-method";
  } catch (e: any) {
    r = "THROW " + e.name;
  }
  console.log(name, which, "x=" + o.x, "seen=" + o.seen, r, o instanceof C, o instanceof F);
}

describe("plainF", Reflect.construct(C, [1], F));
describe("protoNotObject", Reflect.construct(C, [2], G));
describe("self", Reflect.construct(C, [3], C));
describe("noNewTarget", Reflect.construct(C, [4]));
describe("subclassNT", Reflect.construct(C, [5], D));
describe("otherClassNT", Reflect.construct(C, [6], Other));
describe("new", new C(7));
describe("derivedTargetF", Reflect.construct(D, [8], F));
const arrow = () => 0;
try {
  Reflect.construct(C, [9], arrow as any);
  console.log("arrow: no throw");
} catch (e: any) {
  console.log("arrow:", e.name);
}
// The prototype is installed before the body: a body that reads it sees F.prototype.
class Probe {
  p: string;
  constructor() {
    const proto = Object.getPrototypeOf(this);
    this.p = proto === F.prototype ? "F" : proto === Probe.prototype ? "Probe" : "?";
  }
}
console.log("inbody", (Reflect.construct(Probe, [], F) as any).p, new Probe().p);
