// #10661: Perry's `new Function` runtime interpreter (#6559) did not support
// class expressions, so `mysql2` compiled from source but crashed at runtime
// with "unsupported construct: class expression" — `mysql2`'s row parsers
// are built at runtime by `generate-function`
// (`Function.apply(null, keys.concat(src)).apply(null, vals)`,
// generate-function/index.js:172) and the generated source is a class
// expression.
//
// This mirrors the EXACT shape captured from a live `mysql2` `SELECT`
// against a real server (`lib/parsers/text_parser.js`'s `compile()`):
//
//   (function anonymous(wrap, LocalDate) {
//     return ((function () {
//       return class TextRow {
//         constructor(fields) {}
//         next(packet, fields, options) { ... }
//       };
//     })())
//   })
//
// plus the rest of the #10661 supported subset (constructor + regular
// instance/static methods, string/numeric keys, named self-reference) —
// everything the class expression form does NOT need (`extends`,
// getters/setters, private members, computed keys, class fields) is
// out of scope and stays untouched by this test.
//
// `genfun()` below is a minimal stand-in for `generate-function`'s own
// `genfun()`: `toFunction` assembles `"return (" + <pushed lines> + ")"` and
// runs it through `Function.apply(null, keys.concat(src)).apply(null, vals)`
// — verbatim generate-function/index.js:154-172. Every `gen(...)` chain below
// therefore supplies the BODY of that one implicit `return (...)`, so a chain
// only writes its own `return` when it is inside a nested function scope
// (block 1 and 4's inner IIFE) — never at the outer level, which is exactly
// how mysql2's real `text_parser.js`/`binary_parser.js` codegen is shaped.

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

// 1. mysql2's row-parser shape verbatim: a nested IIFE returning a class
// expression with a constructor and one instance method.
{
  const gen = genfun();
  gen("(function () {")("return class TextRow {")("constructor(fields) {")(
    "this.fields = fields;"
  )("}")("next(extra) {")("return this.fields + extra;")("}")("};")("})()");
  const TextRow = gen.toFunction({});
  const row = new TextRow(3);
  console.log("mysql2-row-parser", typeof TextRow, row.next(4));
}

// 2. A named class expression with constructor + multiple instance methods,
// built through the same `Function.apply` machinery, static method
// referencing the class by its own name, and string/numeric method keys.
{
  const gen = genfun();
  gen("class Counter {")("constructor(start) {")("this.n = start;")("}")(
    "inc() {"
  )("this.n = this.n + 1;")("return this.n;")("}")("static make(start) {")(
    "return new Counter(start);"
  )("}")('"label"() {')('return "counter";')("}")("0() {")(
    'return "zero-key";'
  )("}")("}");
  const Counter = gen.toFunction({});
  const a = new Counter(10);
  const b = Counter.make(100);
  console.log("counter-a", a.inc(), a.inc(), a.inc());
  console.log("counter-b", b.inc(), b.inc());
  console.log("counter-a-again", a.inc());
  console.log("counter-label", a["label"]());
  console.log("counter-zero-key", a[0]());
}

// 3. No explicit constructor — the default (empty) constructor.
{
  const gen = genfun();
  gen("class Empty {")("set(v) { this.v = v; return this; }")(
    "get() { return this.v; }"
  )("}");
  const Empty = gen.toFunction({});
  const e = new Empty();
  console.log("empty-ctor", e.set(42).get());
}

// 4. A row-parser-shaped class over multiple synthetic fields, matching the
// per-field member assignment `text_parser.js` actually generates.
{
  const gen = genfun();
  gen("(function () {")("return class Row {")("constructor(fields) {")("}")(
    "next(values) {"
  )("var result = {};")('result["id"] = values[0];')(
    'result["name"] = values[1];'
  )("return result;")("}")("};")("})()");
  const Row = gen.toFunction({});
  const row = new Row([1, 2]);
  console.log("row-parser-fields", JSON.stringify(row.next([7, "ann"])));
}
