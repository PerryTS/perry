// Receivers a class-method site must NOT serve from a remembered receiver
// shape, driven so that the site really is a class-method site: every call goes
// through `drive`, which calls the site function indirectly (a directly called
// tiny function is inlined and re-typed by its argument, which turns the site
// into a dynamic call and tests nothing).
//
//   A. a receiver shaped like the class that carries an own property shadowing
//      the method (a function, a non-callable value, an accessor);
//   B. a receiver of a different class whose layout is identical;
//   C. a method replaced on the prototype after the site cached its receivers.
//
// Each scenario uses its own method name: prototype-replacement bookkeeping is
// per method name, and sharing a name would let one scenario retire another
// scenario's fast path before it is exercised.

function drive(fn: (x: any) => string, x: any, n: number): string {
  let r = "";
  for (let i = 0; i < n; i++) r += fn(x) + ",";
  return r;
}
function attempt(fn: (x: any) => string, x: any): string {
  try { return fn(x); } catch (e) { return "threw " + (e instanceof TypeError ? "TypeError" : "other"); }
}
let out = "";
function say(label: string, v: string) { out += label + "=" + v + "\n"; }

// ---- A. own property shadows the method on a receiver that left the birth shape
class Sh {
  [key: string]: any;
  constructor(kind: number) {
    this.n = 1;
    if (kind === 1) { this.z1 = 1; (this as any).whoA = () => "own-fn"; }
    if (kind === 2) { this.z2 = 1; (this as any).whoA = 42; }
    if (kind === 3) { this.z3 = 1; Object.defineProperty(this, "whoA", { get() { return () => "own-getter"; }, configurable: true }); }
    if (kind === 4) { this.z4 = 1; this.z5 = 2; }
  }
  whoA(): string { return "proto-A"; }
}
function callA(x: Sh): string { return x.whoA(); }
say("A warm", drive(callA, new Sh(0), 1000).length + "");
say("A own fn", drive(callA, new Sh(1), 4));
say("A own number", [0, 1, 2, 3].map(() => attempt(callA, new Sh(2))).join("|"));
say("A own number same receiver", (() => { const r = new Sh(2); return [0, 1, 2, 3].map(() => attempt(callA, r)).join("|"); })());
say("A own getter", drive(callA, new Sh(3), 4));
say("A off-birth, no shadow", drive(callA, new Sh(4), 4));
say("A birth again", drive(callA, new Sh(0), 2));

// ---- B. a different class with a layout identical to the declared class
class P {
  [key: string]: any;
  constructor(many: boolean) { this.n = 1; this.p = 2; if (many) { this.q = 3; this.r = 4; } }
  whoB(): string { return "P" + this.n; }
}
class Q {
  [key: string]: any;
  constructor(many: boolean) { this.n = 10; this.p = 20; if (many) { this.q = 30; this.r = 40; } }
  whoB(): string { return "Q" + this.n; }
}
function callB(x: P): string { return x.whoB(); }
say("B warm", drive(callB, new P(false), 1000).length + "");
say("B P many", drive(callB, new P(true), 3));
say("B Q few", drive(callB, new Q(false), 3));
say("B Q many", drive(callB, new Q(true), 3));
say("B P many after Q", drive(callB, new P(true), 3));

// The same, with the foreign class reaching a site before any declared-class
// receiver left the birth shape.
class P2 {
  [key: string]: any;
  constructor(many: boolean) { this.n = 1; if (many) { this.p = 2; this.q = 3; } }
  whoC(): string { return "P2-" + this.n; }
}
class Q2 {
  [key: string]: any;
  constructor(many: boolean) { this.n = 7; if (many) { this.p = 8; this.q = 9; } }
  whoC(): string { return "Q2-" + this.n; }
}
function callC(x: P2): string { return x.whoC(); }
say("B2 Q first", drive(callC, new Q2(true), 3));
say("B2 P many", drive(callC, new P2(true), 3));
say("B2 Q many", drive(callC, new Q2(true), 2));
say("B2 P few", drive(callC, new P2(false), 2));

// ---- C. method replaced on the prototype after the site cached its receivers
class R {
  [key: string]: any;
  constructor(many: boolean) { this.n = 1; if (many) { this.p = 2; this.q = 3; this.r = 4; } }
  whoD(): string { return "R-original"; }
}
function callD(x: R): string { return x.whoD(); }
const rFew = new R(false), rMany = new R(true);
say("C cached", drive(callD, rFew, 1000).length + ":" + drive(callD, rMany, 4));
R.prototype.whoD = function () { return "R-assigned"; };
say("C assigned", drive(callD, rFew, 2) + drive(callD, rMany, 3) + drive(callD, new R(true), 2));
Object.defineProperty(R.prototype, "whoD", { value: function () { return "R-defined"; }, configurable: true, writable: true });
say("C defined", drive(callD, rFew, 2) + drive(callD, rMany, 3));
delete (R.prototype as any).whoD;
say("C deleted", attempt(callD, rFew) + "|" + attempt(callD, rMany) + "|" + attempt(callD, rMany));
(R.prototype as any).whoD = () => "R-restored";
say("C restored", drive(callD, rFew, 1) + drive(callD, rMany, 2));

console.log(out);
