// #10422: `Function(...)` called without `new` — directly, through
// `Function.apply` and through `Function.call` — with a body built at runtime.
// These call forms compiled to a stub that always threw "new Function() cannot
// run in an ahead-of-time compiled binary", while `new Function(...)` with the
// same arguments ran on the runtime interpreter.
const body = ["return a", "+", "b"].join(" ");

function show(label: string, make: () => any): void {
  try {
    const f = make();
    console.log(label, typeof f, typeof f === "function" ? f(2, 3) : String(f));
  } catch (e) {
    console.log(label, "threw", (e as Error).name);
  }
}

show("new Function(a, b, body)", () => new Function("a", "b", body));
show("Function(a, b, body)", () => Function("a", "b", body));
show("Function(body)", () => Function(["return", "7"].join(" ")));
show("Function.apply(null, [a, b, body])", () => Function.apply(null, ["a", "b", body]));
show("Function.call(null, a, b, body)", () => Function.call(null, "a", "b", body));
show("Function.apply, runtime list", () => {
  const keys = "a b".split(" ");
  return Function.apply(undefined, keys.concat(body));
});
show("Function(...spread)", () => Function(...["a", "b", body]));
show("Function.call(null, ...spread)", () => Function.call(null, ...["a", "b", body]));
show("Function.apply(null)", () => (Function as any).apply(null));
show("syntax error", () => Function("a", ["return", ")"].join(" ")));

// generate-function 2.3.1 `toFunction`, which mysql2 3.23.2 uses for every
// row parser (`generate-function/index.js:172`):
//   return Function.apply(null, keys.concat(src)).apply(null, vals)
function genfun() {
  const lines: string[] = [];
  const gen: any = function (line: string) {
    lines.push(line);
    return gen;
  };
  gen.toFunction = function (scope: any) {
    const src = "return (" + lines.join("\n") + ")";
    const keys = Object.keys(scope || {});
    const vals = keys.map((key) => scope[key]);
    return Function.apply(null, keys.concat(src)).apply(null, vals);
  };
  return gen;
}
const gen = genfun();
gen("function add(a, b) {")("return a + b + offset")("}");
const add = gen.toFunction({ offset: 100 });
console.log("generate-function", typeof add, add(2, 3));

// A row parser in the same style: generated code over a runtime column list.
const columns = ["id", "name", "score"];
const parserSource = [
  "var row = {};",
  ...columns.map((c, i) => "row[" + JSON.stringify(c) + "] = values[" + i + "];"),
  "return row;",
].join("\n");
const parseRow = Function.apply(null, ["values", parserSource]);
console.log("row", JSON.stringify(parseRow([1, "ann", 9.5])));
