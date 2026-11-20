// The first `f.call(obj, ..)` / `f.apply(obj, ..)` of a program, read inside a
// function before any code has named Function.prototype, answers the realm's
// own `call` / `apply` (the prototype is born on that read).
var hop = function (this: any, k: string): string { return "s" + k + ":" + this.n; };
var cache = { n: 1, m: 2 };
const t = (n: string, f: () => any) => {
  try {
    console.log(n, f());
  } catch (e) {
    console.log(n, "E", String(e));
  }
};
t("call-first", () => hop.call(cache, "z"));
t("apply-first", () => hop.apply(cache, ["y"]));
t("call-again", () => hop.call({ n: 3 }, "x"));
t("call-null", () => hop.call(null as any, "w"));
t("bind", () => hop.bind(cache, "v")());
t("call-is-proto", () => (hop.call === Function.prototype.call) + " " + (hop.apply === Function.prototype.apply));
