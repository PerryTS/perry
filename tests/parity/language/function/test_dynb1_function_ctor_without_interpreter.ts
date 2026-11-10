// A program with no eval, no `new Function`, no node:vm and no value use of
// `Function` links no interpreter. A function's constructor reached under a
// computed key is then the Function constructor, which converts its
// arguments to strings (running their toString) and throws a TypeError,
// running no program code (six arguments of the depd wrapfunction shape make
// a runtime function instead); the async and generator function constructors
// run no program code either (wpfacts: "without an interpreter the Function
// constructor runs nothing").
function pick(o: any, k: string): any {
  return o[k];
}
const key = ["constr", "uctor"].join("");
let touched = 0;
const arg = {
  toString() {
    touched++;
    return "return 1";
  },
};

function show(label: string, run: () => any): void {
  touched = 0;
  try {
    const v = run();
    console.log(label, "returned", typeof v, "toString calls", touched);
  } catch (e) {
    console.log(label, "threw", (e as Error).name, "toString calls", touched);
  }
}

const F = pick(function () {}, key);
show("call", () => F(arg));
show("call two args", () => F(arg, arg));
show("construct", () => new F(arg));
show("as a method", () => {
  const holder: any = { m: F };
  return holder.m(arg);
});
show("symbol argument", () => F(Symbol("s")));
show("call no args", () => F());
show("depd shape", () =>
  F("fn", "log", "deprecate", "message", "site",
    "return function (x) { log.call(deprecate, message, site)\nreturn fn.apply(this, arguments)\n}"));
const AF = pick(async function () {}, key);
show("async call", () => AF(arg));
show("async construct", () => new AF(arg));
const GF = pick(function* () {}, key);
show("generator call", () => GF(arg));
show("generator construct", () => new GF(arg));
const AGF = pick(async function* () {}, key);
show("async generator call", () => AGF(arg));
show("async generator construct", () => new AGF(arg));
