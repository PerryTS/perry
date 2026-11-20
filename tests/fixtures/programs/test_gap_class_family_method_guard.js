// A class born on its family layout (object model case 2c) is called through
// the family arm of the method-direct guard; every receiver that must not take
// that arm (own or accessor shadow, swapped prototype, subclass, patched
// prototype method, method called before the constructor stores its keys)
// still dispatches by name.
class P {
  constructor(early) { if (early) this.log.push("early:" + this.who()); this.a = 1; this.b = 2; }
  who() { return "P" + this.a; }
  call() { return this.who(); }
}
P.prototype.log = [];
const out = [];
function run(o) { let s = ""; for (let i = 0; i < 3; i++) s += o.call() + ","; return s; }
const p1 = new P(false); out.push(run(p1));
// own property shadowing the method
const p2 = new P(false); p2.who = function () { return "own" + this.b; }; out.push(run(p2));
// accessor own property shadowing the method
const p3 = new P(false); Object.defineProperty(p3, "who", { get() { return () => "acc"; } }); out.push(run(p3));
// prototype swapped per object
class Q { who() { return "Q"; } call() { return this.who(); } }
const p4 = new P(false); Object.setPrototypeOf(p4, Q.prototype); out.push(run(p4));
// method called during the constructor, before its keys are stored
const p5 = new P(true); out.push(P.prototype.log.join("|"));
// subclass instance through the base method
class S extends P { who() { return "S" + this.b; } }
out.push(run(new S(false)));
// deleted / frozen receivers
const p6 = new P(false); delete p6.a; out.push(run(p6));
const p7 = Object.freeze(new P(false)); out.push(run(p7));
// extra key added after construction
const p8 = new P(false); p8.c = 3; out.push(run(p8));
// prototype method replaced after warm calls
const p9 = new P(false); out.push(run(p9)); P.prototype.who = function () { return "patched" + this.a; }; out.push(run(p9)); out.push(run(p1));
console.log(out.join("\n"));
