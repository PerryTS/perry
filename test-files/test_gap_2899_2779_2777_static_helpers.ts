// Gap test: RegExp.escape (#2899), Map.groupBy (#2779), Object.groupBy (#2777)

// ---- RegExp.escape (#2899) ----
console.log(RegExp.escape("a.b[c]"));
console.log(RegExp.escape("abc"));
console.log(RegExp.escape("1abc"));
console.log(RegExp.escape("^$\\.*+?()[]{}|"));
console.log(RegExp.escape("/"));
console.log(RegExp.escape(" "));
console.log(RegExp.escape("\t\n\r"));
console.log(RegExp.escape("_-,=<>#&!%:;@~'`\""));
console.log(new RegExp(RegExp.escape("a.b[c]")).test("a.b[c]"));
console.log(new RegExp(RegExp.escape("a.b[c]")).test("axb[c]"));

// ---- Object.groupBy (#2777) ----
const og1 = Object.groupBy([1, 2, 3], (v) => (v % 2 ? "odd" : "even"));
console.log(JSON.stringify(og1));
const og2 = Object.groupBy(new Set([1, 2, 3]), (v) => (v > 1 ? "gt1" : "one"));
console.log(JSON.stringify(og2));
const og3 = Object.groupBy("aba", (ch) => ch);
console.log(JSON.stringify(og3));
console.log(Object.getPrototypeOf(Object.groupBy([1], () => "x")) === null);
console.log(Object.keys(Object.groupBy([1], () => "x")).join(","));

const symG = Symbol("g");
const ogSym = Object.groupBy([1], () => symG);
console.log(Object.getOwnPropertySymbols(ogSym)[0] === symG);
console.log(Object.keys(ogSym).length);
console.log(JSON.stringify(ogSym[symG]));

// ---- Map.groupBy (#2779) ----
const mg1 = Map.groupBy([1, 2, 3], (v) => (v % 2 ? "odd" : "even"));
console.log(mg1 instanceof Map);
console.log(JSON.stringify([...mg1.entries()]));
console.log(
  JSON.stringify([...Map.groupBy(new Set([1, 2, 3]), (v) => (v > 1 ? "gt1" : "one")).entries()]),
);
console.log(JSON.stringify([...Map.groupBy("aba", (ch) => ch).entries()]));

const symM = Symbol("s");
const mgSym = Map.groupBy([1], () => symM);
console.log([...mgSym.keys()][0] === symM);
console.log(JSON.stringify(mgSym.get(symM)));

// numeric (non-string) keys preserved without coercion
const mgNum = Map.groupBy([10, 20, 11], (v) => Math.floor(v / 10));
console.log(JSON.stringify([...mgNum.entries()]));

// ---- TypeError cases ----
try {
  Object.groupBy(null, (x) => x);
} catch (e) {
  console.log("og null:", e instanceof TypeError);
}
try {
  (Map.groupBy as any)([1], 1);
} catch (e) {
  console.log("mg bad cb:", e instanceof TypeError);
}
