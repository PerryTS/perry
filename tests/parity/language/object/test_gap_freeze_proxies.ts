function result(f: () => any) { try { f(); return "ok"; } catch (e: any) { return e.name; } }
for (let frozen = 0; frozen < 2; frozen++) {
  const log: string[] = [];
  const sym = Symbol.for("proxy");
  const base: any = { x: 3 }; base[sym] = 5;
  let v = 7;
  Object.defineProperty(base, "access", { get() { return v; }, set(n) { v = n; }, configurable: true });
  const p = new Proxy(base, {
    preventExtensions(t) { log.push("prevent"); return Reflect.preventExtensions(t); },
    ownKeys(t) { log.push("keys"); return Reflect.ownKeys(t); },
    getOwnPropertyDescriptor(t, k) { log.push("get " + String(k)); return Reflect.getOwnPropertyDescriptor(t, k); },
    defineProperty(t, k, d) { log.push("define " + String(k) + " " + d.configurable + " " + d.writable); return Reflect.defineProperty(t, k, d); },
    isExtensible(t) { log.push("extensible"); return Reflect.isExtensible(t); }
  });
  if (frozen) Object.freeze(p); else Object.seal(p);
  console.log("traps", frozen, log.join("|"));
  log.length = 0;
  console.log("status", Object.isFrozen(p), Object.isSealed(p), Object.isExtensible(p), v);
  console.log("test-traps", log.join("|"));
  console.log("target", Object.isFrozen(base), Object.isSealed(base), Reflect.set(p, "x", 11));
}
const rejected = new Proxy({}, { preventExtensions() { return false; } });
console.log("reject", result(() => Object.freeze(rejected)), result(() => Object.seal(rejected)),
  result(() => Object.preventExtensions(rejected)));
const rejectedDefine = new Proxy({ x: 1 }, {
  preventExtensions(t) { return Reflect.preventExtensions(t); },
  defineProperty() { return false; }
});
console.log("reject-define", result(() => Object.freeze(rejectedDefine)));
