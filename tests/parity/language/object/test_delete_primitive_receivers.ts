// `delete` on a primitive base: ToObject's wrapper answers [[Delete]]. A Symbol wrapper owns nothing (`delete sym.x`
// aborted the process: the Symbol cell reached the object delete path as an ObjectHeader); a String wrapper owns
// `length` and one index per UTF-16 unit, all non-configurable: `false` in sloppy code, a TypeError in strict code
// (was `true`). Controls: other keys on a String, Number / Boolean / BigInt bases, Symbol-keyed deletes on objects,
// frozen objects, null / undefined bases (TypeError).
const SY = Symbol("sy");
function sloppy(o: any, k: any): string {
  try {
    return String(delete o[k]);
  } catch (e: any) {
    return "THROW " + e.name;
  }
}
function strict(o: any, k: any): string {
  "use strict";
  try {
    return String(delete o[k]);
  } catch (e: any) {
    return "THROW " + e.name;
  }
}
function strictDot(o: any): string {
  "use strict";
  try {
    return String(delete o.length) + "/" + String(delete o.x);
  } catch (e: any) {
    return "THROW " + e.name;
  }
}
function sloppyDot(o: any): string {
  return String(delete o.length) + "/" + String(delete o.x) + "/" + String(delete o.description);
}
const bases: [string, () => any][] = [
  ["symbol", () => Symbol("prim")],
  ["wellknown", () => Symbol.iterator],
  ["sso", () => "abc"],
  ["heapstr", () => "a string well past the inline limit"],
  ["empty", () => ""],
  ["number", () => 5],
  ["bool", () => true],
  ["bigint", () => BigInt(7)],
  ["null", () => null],
  ["undef", () => undefined],
];
const keys: [string, any][] = [
  ["0", 0],
  ["'1'", "1"],
  ["2.0", 2.0],
  ["-0", -0],
  ["3", 3],
  ["'01'", "01"],
  ["1.5", 1.5],
  ["length", "length"],
  ["x", "x"],
  ["sym", SY],
  ["iter", Symbol.iterator],
];
for (const [bn, mk] of bases) {
  let line = bn + ":";
  for (const [kn, k] of keys) line += " " + kn + "=" + sloppy(mk(), k) + "|" + strict(mk(), k);
  console.log(line);
  let dot: string;
  try {
    dot = sloppyDot(mk());
  } catch (e: any) {
    dot = "THROW " + e.name;
  }
  console.log("  dot " + dot + " strict " + strictDot(mk()));
}
// Symbol-keyed deletes on objects stay as they were.
const o: any = { a: 1 };
o[SY] = 2;
o[Symbol.iterator] = 3;
console.log("obj", strict(o, SY), sloppy(o, Symbol.iterator), SY in o, Symbol.iterator in o, Object.getOwnPropertySymbols(o).length);
const f: any = Object.freeze({ a: 1, [SY]: 2 });
console.log("frozen", sloppy(f, SY), strict(f, SY), sloppy(f, "a"), strict(f, "zz"));
const wrapped: any = new String("ab");
console.log("wrapper", sloppy(wrapped, 0), sloppy(wrapped, "length"), sloppy(wrapped, 5));
