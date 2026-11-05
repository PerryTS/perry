// Spread is GetIterator (owner 2026-11-04): a nullish, primitive non-string or non-iterable
// object operand throws TypeError in calls, method calls, new, super(...) and array literals.
function f(...a) { return a.length + ":" + a.join(","); }
function g(a, b) { return a + "," + b; }
class V { set(...a) { return a.length + ":" + a.join(","); } fixed(a, b) { return a + "/" + b; } }
class B { constructor(...a) { this.n = a.length; } }
class D extends B { constructor(x) { super(...x); } }
const v = new V();
const und = undefined, nul = null, num = 5, obj = { length: 2, 0: "a", 1: "b" };
function t(label, fn) { try { console.log(label, fn()); } catch (e) { console.log(label, "throws", e.constructor.name); } }
t("undef", () => f(...und, 1));
t("null", () => f(1, ...nul));
t("num", () => f(...num));
t("arraylike", () => f(...obj, "z"));
t("fixed-undef", () => g(...und));
t("m-undef", () => v.set(...und, 1));
t("m-arraylike", () => v.set(...obj));
t("m-fixed-null", () => v.fixed(...nul));
t("max-undef", () => Math.max(...und, 3));
t("new-undef", () => new B(...und).n);
t("super-null", () => new D(null).n);
t("lit-obj", () => [...obj].length);
t("str-ok", () => f(..."ab"));
t("apply-undef-ok", () => String.fromCharCode.apply(null, undefined).length);
