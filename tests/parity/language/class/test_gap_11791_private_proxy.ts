// #11791: a proxy has none of its target's private elements (node): `#x in
// proxy` is false and a private access through a proxy throws.
class C {
  #x = 1;
  #m() { return 2; }
  get #g() { return 3; }
  static #s = 4;
  static has(o: any) { return #x in o; }
  static hasM(o: any) { return #m in o; }
  static read(o: any) { return o.#x; }
  static write(o: any) { o.#x = 5; }
  static call(o: any) { return o.#m(); }
  static getter(o: any) { return o.#g; }
  static stat(o: any) { return o.#s; }
}
const t = (label: string, f: () => unknown) => {
  try { console.log(label, f()); } catch (e: any) { console.log(label, e.constructor.name + ": " + e.message); }
};
const c = new C();
const p = new Proxy(c, {});
const traps: string[] = [];
const loud = new Proxy(c, { get(tg, k, r) { traps.push(String(k)); return Reflect.get(tg, k, r); }, has(tg, k) { traps.push("has " + String(k)); return Reflect.has(tg, k); } });
t("in:", () => [C.has(p), C.hasM(p), C.has(loud), C.has(c)]);
t("read:", () => C.read(p));
t("write:", () => C.write(p));
t("call:", () => C.call(p));
t("getter:", () => C.getter(p));
t("static:", () => C.stat(new Proxy(C, {})));
t("target ok:", () => [C.read(c), C.call(c), C.getter(c)]);
t("revoked:", () => { const r = Proxy.revocable(c, {}); r.revoke(); return C.has(r.proxy); });
console.log("traps:", traps);
