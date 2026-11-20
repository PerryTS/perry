// #11791: every private-element TypeError carries node's message.
class C {
  #x = 1;
  #m() {}
  get #g() { return 1; }
  set #s(v: number) {}
  static #sx = 1;
  static #sm() { return 1; }
  static r(o: any) { return o.#x; }
  static w(o: any) { o.#x = 2; }
  static c(o: any) { o.#m(); }
  static g(o: any) { return o.#g; }
  static ss(o: any) { o.#s = 1; }
  static wg(o: any) { o.#g = 1; }
  static rs(o: any) { return o.#s; }
  static wm(o: any) { o.#m = 1; }
  static sr(o: any) { return o.#sx; }
  static sw(o: any) { o.#sx = 2; }
  static sc(o: any) { return o.#sm(); }
  static upd(o: any) { o.#x++; }
}
class B { constructor(o: any) { return o; } }
class F extends B { #f = 1; }
class M extends B { #mm() {} }
const t = (label: string, f: () => void) => {
  try { f(); console.log(label, "ok"); } catch (e: any) { console.log(label, e.constructor.name + ": " + e.message); }
};
t("read foreign:", () => C.r({}));
t("write foreign:", () => C.w({}));
t("update foreign:", () => C.upd({}));
t("call foreign:", () => C.c({}));
t("getter foreign:", () => C.g({}));
t("setter foreign:", () => C.ss({}));
t("write getter-only:", () => C.wg(new C()));
t("read setter-only:", () => C.rs(new C()));
t("write method:", () => C.wm(new C()));
t("static read foreign:", () => C.sr({}));
t("static read class:", () => C.sr(class {}));
t("static write foreign:", () => C.sw({}));
t("static call foreign:", () => C.sc({}));
t("read primitive:", () => C.r(1));
t("read null:", () => C.r(null));
const o = {};
new F(o);
t("field twice:", () => new F(o));
const q = {};
new M(q);
t("methods twice:", () => new M(q));
t("ok:", () => { C.r(new C()); C.w(new C()); C.c(new C()); C.ss(new C()); });

// Nullish and primitive receivers.
class N {
  #x = 1;
  #m() {}
  get #g() { return 1; }
  static #s = 1;
  static #sm() {}
  static r(o: any) { return o.#x; }
  static w(o: any) { o.#x = 2; }
  static c(o: any) { o.#m(); }
  static g(o: any) { return o.#g; }
  static wg(o: any) { o.#g = 1; }
  static h(o: any) { return #x in o; }
  static hm(o: any) { return #m in o; }
  static s(o: any) { return o.#s; }
  static ws(o: any) { o.#s = 2; }
  static sm(o: any) { o.#sm(); }
}
for (const v of [null, undefined, 1, "s", true]) {
  const l = String(v);
  t(l + " read:", () => N.r(v));
  t(l + " write:", () => N.w(v));
  t(l + " call:", () => N.c(v));
  t(l + " getter:", () => N.g(v));
  t(l + " setter:", () => N.wg(v));
  t(l + " in:", () => N.h(v));
  t(l + " in method:", () => N.hm(v));
  t(l + " static:", () => N.s(v));
  t(l + " static write:", () => N.ws(v));
  t(l + " static call:", () => N.sm(v));
}
