// f(...inst) where inst's class declares *[Symbol.iterator](): the spread calls the member and drains the generator exactly as node does.
const log = [];
function f(...a) { return a.join(","); }
function g(a, b, c, d) { return [a, b, c, d].join(","); }

class Vec3 {
  constructor(x, y, z) { this.x = x; this.y = y; this.z = z; }
  *[Symbol.iterator]() { yield this.x; yield this.y; yield this.z; }
}
const v = new Vec3(1, 2, 3);
console.log("plain", f(...v), g(...v, 4), g(0, ...v), [...v].length);
let s = 0;
for (let i = 0; i < 1000; i++) { v.x = i; s += g(...v, 1).length; }
console.log("loop", s);

// The generator object: its prototype is the member's .prototype.
class Probe {
  *[Symbol.iterator]() { yield Object.getPrototypeOf(this) === Probe.prototype; }
}
const p = new Probe();
console.log("this", f(...p));

// A throwing next(): the error propagates, return() is never called.
class Thrower {
  constructor() { this.closed = false; }
  *[Symbol.iterator]() {
    try { yield 1; throw new Error("boom"); } finally { this.closed = true; log.push("finally"); }
  }
}
const t = new Thrower();
try { f(...t); } catch (e) { console.log("throw", e.message, t.closed, log.join("|")); }

// Early exit inside the generator: try/finally runs on normal completion.
class Early {
  *[Symbol.iterator]() { try { yield 1; return; yield 2; } finally { log.push("early-finally"); } }
}
console.log("early", f(...new Early()), log.join("|"));

// Subclass inheriting, and a subclass overriding.
class Vec3b extends Vec3 {}
class Vec3r extends Vec3 { *[Symbol.iterator]() { yield this.z; yield this.y; yield this.x; } }
const vb = new Vec3b(4, 5, 6), vr = new Vec3r(7, 8, 9);
console.log("inherit", f(...vb), "override", f(...vr));

// A binding first holding a Vec3, then a subclass instance with its own member.
let w = new Vec3(5, 6, 7);
const outs = [];
for (let i = 0; i < 3; i++) { outs.push(f(...w)); w = i === 0 ? new Vec3r(1, 2, 3) : new Vec3b(4, 4, 4); }
console.log("rebind", outs.join("|"));

// An own [Symbol.iterator] on one instance.
const own = new Vec3(1, 1, 1);
own[Symbol.iterator] = function* () { yield "own"; };
console.log("own", f(...own), f(...v).split(",").length);

// %GeneratorPrototype%.next patched: the drain calls the patched next.
class Gp { *[Symbol.iterator]() { yield 1; yield 2; } }
const GenProto = Object.getPrototypeOf(Object.getPrototypeOf((new Gp())[Symbol.iterator]()));
const realNext = GenProto.next;
GenProto.next = function (...a) { const r = realNext.apply(this, a); if (!r.done) r.value *= 10; return r; };
console.log("patched-next", f(...new Gp()));
GenProto.next = realNext;
console.log("restored-next", f(...new Gp()));

// The member replaced on the prototype after the class (r2's case): generic path, still correct.
class Late { constructor() { this.a = 1; } *[Symbol.iterator]() { yield "old"; } }
const late = new Late();
console.log("before", f(...late));
Late.prototype[Symbol.iterator] = function* () { yield "new"; yield this.a; };
console.log("after", f(...late));

// Replaced by a getter on the prototype (defineProperty).
class Acc { *[Symbol.iterator]() { yield "acc-old"; } }
const acc = new Acc();
Object.defineProperty(Acc.prototype, Symbol.iterator, { get() { log.push("getter"); return function* () { yield "acc-new"; }; }, configurable: true });
console.log("getter", f(...acc), log.includes("getter"));

// A member returning a non-iterator / non-object.
class Bad { *[Symbol.iterator]() { yield 1; } }
const bad = new Bad();
Bad.prototype[Symbol.iterator] = function () { return 5; };
try { f(...bad); console.log("bad no-throw"); } catch (e) { console.log("bad", e instanceof TypeError); }

// Spread of a non-iterable class instance and of null.
class Plain { constructor() { this.q = 1; } }
try { f(...new Plain()); console.log("plain no-throw"); } catch (e) { console.log("noniter", e instanceof TypeError); }
try { const z = null; f(...z); } catch (e) { console.log("null", e instanceof TypeError); }

// Spread into new and into a method call.
class Pt { constructor(a, b, c) { this.s = a + b + c; } }
class Holder { sum(a, b, c, d) { return a + b + c + d; } }
console.log("new", new Pt(...v).s > 0, "method", new Holder().sum(...new Vec3(1, 2, 3), 4));
console.log("done");
