// Detached, borrowed, and simultaneously retained class evaluations.
class P { static make() { return "P"; } }
class S extends P { static make() { return super.make(); } }
const detached = S.make;
try { console.log("detached", detached()); } catch (e) { console.log("detached", e.name); }
function parents() { return class { m() { return "inst"; } static m() { return "static"; } }; }
const Parent = parents();
class Child extends Parent { m() { return super.m(); } }
console.log("borrowed", Child.prototype.m.call(Child));
function mk(Base) { return class extends Base { m() { return super.m(); } arrow() { return () => super.m(); } }; }
class X { m() { return "X"; } }
class Y { m() { return "Y"; } }
const A = mk(X), B = mk(Y);
console.log("retained", A.prototype.m.call({}), B.prototype.m.call({}));
const ai = new A(), bi = new B();
console.log("retained instances", ai.m(), bi.m());
const arrow = A.prototype.arrow.call({});
Object.setPrototypeOf(A.prototype, Y.prototype);
console.log("relinked", A.prototype.m.call({}), arrow(), B.prototype.m.call({}), ai.m());
Object.setPrototypeOf(S, { make() { return "new"; } });
console.log("static relink", detached());

function staticFactory(Base) { return class extends Base { static m() { return super.m(); } }; }
class SX { static m() { return "SX"; } }
class SY { static m() { return "SY"; } }
const SA = staticFactory(SX), SB = staticFactory(SY);
const sam = SA.m, sbm = SB.m;
console.log("static retained", sam(), sbm(), SA.m(), SB.m());
class Args { m(...values) { return values.join(","); } }
class Spread extends Args { m() { return super.m(1, ...[2, 3], 4); } *gen() { yield super.m(5); } }
console.log("spread", new Spread().m());
console.log("generator", new Spread().gen().next().value);
class G { get value() { return this.tag; } }
class R extends G { read() { return super.value; } }
console.log("getter", R.prototype.read.call({tag: "receiver"}));
class NullHome extends null { read() { return super.value; } }
try { NullHome.prototype.read.call({}); } catch(e) { console.log("null", e.name); }
function privateFactory() { return class { #x = 1; m() { return this.#x; } }; }
const PA = privateFactory(), PB = privateFactory();
try { PA.prototype.m.call(new PB()); } catch(e) { console.log("private", e.name); }

class NativeHome extends WeakMap { read(key) { return super.get(key); } }
const nativeKey = {};
const nativeHome = new NativeHome([[nativeKey, 42]]);
console.log("native", nativeHome.read(nativeKey));
