// A program store onto a builtin constructor under a computed key, made
// through a reference the constructor was handed as (`put(Array, k, f)`),
// may replace any of its statics: the direct call `Array.from(…)` then calls
// whatever the property holds (ECMA-262 13.3.6.1), here the program's
// function. perry called the intrinsic: the member read was proven native
// although the program had stored into the constructor.
function put(o: any, k: string, v: any): void {
  o[k] = v;
}
const j = (a: string, b: string): string => a + b;
put(Array, j("fr", "om"), (_x: any) => "program Array.from");
put(Array, j("is", "Array"), (_x: any) => "program Array.isArray");
put(Array, j("o", "f"), (..._x: any[]) => "program Array.of");
put(Object, j("ke", "ys"), (_x: any) => "program Object.keys");
put(Object, j("ent", "ries"), (_x: any) => "program Object.entries");
put(Number, j("isInt", "eger"), (_x: any) => "program Number.isInteger");
put(String, j("fromChar", "Code"), (_x: any) => "program String.fromCharCode");
put(JSON, j("par", "se"), (_x: any) => "program JSON.parse");
put(Math, j("fl", "oor"), (_x: any) => "program Math.floor");
put(Promise, j("res", "olve"), (_x: any) => "program Promise.resolve");
const out: any[] = [
  Array.from([1]),
  Array.isArray([1]),
  Array.of(1),
  Object.keys({ a: 1 }),
  Object.entries({ a: 1 }),
  Number.isInteger(1),
  String.fromCharCode(65),
  JSON.parse("1"),
  Math.floor(1.5),
  Promise.resolve(1),
];
for (const v of out) console.log(typeof v === "string" ? v : "intrinsic " + JSON.stringify(v));
