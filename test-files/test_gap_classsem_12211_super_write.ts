class A { set x(v) { this.log = "A" + v; } }
class B { set x(v) { this.log = "B" + v; } }
class C extends A { w() { super.x = 1; return this.log; } }
Object.setPrototypeOf(C.prototype, B.prototype);
console.log("relinked", new C().w());
class Data { w(v) { return super.x = v; } }
const d = new Data(); console.log("data", d.w(4), d.x, Object.hasOwn(d, "x"));
Object.defineProperty(Object.getPrototypeOf(Data.prototype), "locked", {value: 1, writable: false, configurable: true});
class Locked { w() { super.locked = 3; } }
try { new Locked().w(); } catch(e) { console.log("readonly", e.name); }
delete Object.getPrototypeOf(Data.prototype).locked;
class Static extends A { static w() { super.x = 5; return this.log; } }
Object.setPrototypeOf(Static, { set x(v) { this.log = "S" + v; } });
console.log("static", Static.w());
class Order extends A { w() { super.x = (Object.setPrototypeOf(Order.prototype, B.prototype), 7); return this.log; } }
console.log("rhs order", new Order().w(), new Order().w());

class ProxyWrite extends A { w(key, value) { return super[key] = value; } }
const proxyParent = new Proxy({}, { set(target, key, value, receiver) { Object.defineProperty(receiver, "log", { value: key + ":" + value, writable: true, configurable: true }); return true; } });
Object.setPrototypeOf(ProxyWrite.prototype, proxyParent);
const proxyWriter = new ProxyWrite();
console.log("proxy", proxyWriter.w("x", 8), proxyWriter.log);
class NullWrite extends null { w() { super.x = 1; } }
try { NullWrite.prototype.w.call({}); } catch(e) { console.log("null write", e.name); }
class ConstructorWrite extends A { constructor() { super(); super.x = 9; } }
Object.setPrototypeOf(ConstructorWrite.prototype, B.prototype);
console.log("constructor write", new ConstructorWrite().log);
