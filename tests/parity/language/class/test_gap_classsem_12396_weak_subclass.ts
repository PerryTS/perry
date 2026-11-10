const key = {};
class X extends WeakMap {}
const x = new X();
console.log("default proto", Object.getPrototypeOf(x) === X.prototype, x instanceof X, x instanceof WeakMap);
try { x.set(key, 7); console.log("default get", x.get(key)); } catch(e) { console.log("default get", e.name); }
class Y extends WeakMap { constructor() { super(); console.log("target", new.target === Y); this.set(key, 8); } }
try { const y = new Y(); console.log("explicit", Object.getPrototypeOf(y) === Y.prototype, y.get(key)); } catch(e) { console.log("explicit", e.name); }
try { const z = Reflect.construct(WeakMap, [], X); z.set(key, 9); console.log("reflect", Object.getPrototypeOf(z) === X.prototype, z instanceof X, z.get(key)); } catch(e) { console.log("reflect", e.name); }
class WS extends WeakSet {}
const ws = new WS();
try { ws.add(key); console.log("weakset", Object.getPrototypeOf(ws) === WS.prototype, ws instanceof WS, ws.has(key)); } catch(e) { console.log("weakset", e.name); }

function factory(Base) { return class extends Base {}; }
const F = factory(WeakMap);
try { const f = new F(); f.set(key, 10); console.log("factory", Object.getPrototypeOf(f) === F.prototype, f.get(key)); } catch(e) { console.log("factory", e.name); }
const AF = WeakMap;
class Alias extends AF { constructor() { super([[key, 11]]); } }
try { const a = new Alias(); console.log("alias", Object.getPrototypeOf(a) === Alias.prototype, a.get(key)); } catch(e) { console.log("alias", e.name); }

class Branded extends WeakMap {
  #value = 12;
  field = 13;
  constructor() { super(); this.set(key, 14); }
  read() { return this.#value; }
}
const branded = new Branded();
console.log("private receiver", branded.read(), branded.field, branded.get(key), branded instanceof Branded);
