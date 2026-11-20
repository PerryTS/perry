// #10421 / #10423: the `Function` constructor reached as a VALUE, with bodies
// built at runtime. The file has no literal `new Function(...)` or
// `Function(...)` call with a runtime body, so in an auto-optimized build only
// the value uses themselves can keep the runtime interpreter linked (#10421).
// Calling the value without `new` returned `undefined`, and
// `fn.constructor(...)` returned an empty object (#10423).
const body = ["return a", "+", "b"].join(" ");

function show(label: string, make: () => any): void {
  try {
    const f = make();
    console.log(label, typeof f, typeof f === "function" ? f(2, 3) : String(f));
  } catch (e) {
    console.log(label, "threw", (e as Error).name);
  }
}

const F: any = Function;
show("alias new", () => new F("a", "b", body));
show("alias call", () => F("a", "b", body));
show("alias call, constant body", () => F("a", "b", "return a + b"));
show("alias call, one param list", () => F("a, b", body));
show("bind", () => Function.bind(null, "a", "b")(body));
show("Reflect.construct", () => Reflect.construct(Function, ["a", "b", body]));
show("Reflect.apply", () => Reflect.apply(Function, undefined, ["a", "b", body]));
show("computed global key", () => new (globalThis as any)["Func" + "tion"]("a", "b", body));
show("destructured global", () => {
  const { Function: G } = globalThis as any;
  return new G("a", "b", body);
});
show("fn.constructor call", () => (function () {}).constructor("a", "b", body));
show("fn.constructor new", () => new ((function () {}).constructor as any)("a", "b", body));
show("arrow.constructor call", () => ((() => 0) as any).constructor("a", "b", body));
show("getPrototypeOf(fn).constructor", () => {
  const C = Object.getPrototypeOf(function () {}).constructor;
  return C("a", "b", body);
});

// lodash 4.18.1 `runInContext` rebinds the global (`lodash.js:1456`).
function runInContext(context: any) {
  var Function = context.Function;
  return Function("a", "b", body);
}
show("lodash context.Function", () => runInContext(globalThis));

// A CommonJS module whose export is the constructor itself.
const moduleLike: any = { exports: {} };
moduleLike.exports = Function;
show("module.exports = Function", () => moduleLike.exports("a", "b", body));

console.log(
  "identity",
  F === Function,
  (function () {}).constructor === Function,
  typeof F,
  F.name,
  F.length,
);

const twice = F("x", ["return", "x * 2"].join(" "));
console.log(
  "built function",
  twice(21),
  twice.length,
  twice.name,
  twice.call(null, 4),
  twice.apply(null, [5]),
);
show("syntax error", () => F("a", ["return", ")"].join(" ")));
