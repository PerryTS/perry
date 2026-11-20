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

// ---- group arrays survive JSON.stringify (#9849 raw-f64 layout) ----
// The group arrays are built by the runtime with direct slot writes, so the
// "every slot is an unboxed double" layout flag a fresh array is born with has
// to be re-derived from what actually landed in them. When it was not,
// `JSON.stringify` trusted the flag, read each NaN-boxed string as a double and
// emitted `null` — while `arr[0]` and `String(arr)` stayed correct, so only a
// JSON round-trip could see it. Cover every element kind, and pin the
// non-JSON readback beside it so a future failure says which half broke.
const jsonStr = Map.groupBy(["a", "b", "a"], (v) => v);
console.log(JSON.stringify(jsonStr.get("a")), String(jsonStr.get("a")), jsonStr.get("a")![0]);
const jsonBool = Map.groupBy([true, false, true], (v) => v);
console.log(JSON.stringify(jsonBool.get(true)), String(jsonBool.get(true)));
const jsonObj = Map.groupBy([{ a: 1 }, { a: 2 }], (v) => "k");
console.log(JSON.stringify(jsonObj.get("k")));
const jsonMixed = Map.groupBy([1, "two", null, undefined, 3.5], () => "m");
console.log(JSON.stringify(jsonMixed.get("m")));
const jsonNum = Map.groupBy([1, 2, 3], () => "n");
console.log(JSON.stringify(jsonNum.get("n")));
const ogJson = Object.groupBy(["a", "b", "a"], (v) => v);
console.log(JSON.stringify(ogJson.a), JSON.stringify(ogJson));
console.log(JSON.stringify([...Map.groupBy("aba", (ch) => ch).values()]));

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
