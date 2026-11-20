// #10479: `x instanceof C` decoded every NaN-box tag band >= 0x7FF8 as an
// object address. A 1-5 byte inline (SSO) string produced at run time packs its
// bytes plus a length byte into a payload that lands in the heap window, so the
// runtime read a GC header below it and segfaulted (ajv 8 `compile()` ->
// `arg instanceof _Code` on the string "uri"). The same decode turned an INT32
// class ref into an address, and the fall-through class-id walk read an
// array's `length` as a class id. Covers every LHS value kind against class,
// function and builtin right-hand sides, in the static and dynamic forms.
import * as util from "node:util";

const rt = <T>(v: T): T => JSON.parse(JSON.stringify(v));

// --- issue repro --------------------------------------------------------------
class K {}
for (let n = 0; n <= 7; n++) {
  const s = rt("abcdefgh".slice(0, n));
  console.log(`${JSON.stringify(s)} instanceof K:`, s instanceof K);
}
const concat: any = "u" + "ri".slice(0);
console.log("concat instanceof K:", concat instanceof K);
console.log("JSON.parse('\"uri\"') instanceof K:", JSON.parse('"uri"') instanceof K);
console.log("field instanceof K:", JSON.parse('{"f":"uri"}').f instanceof K);

// --- LHS kinds x RHS kinds ------------------------------------------------------
class A {
  x = 1;
}
class B extends A {
  y = 2;
}
function F(this: any) {
  this.f = 1;
}
class Even {
  static [Symbol.hasInstance](v: any) {
    return typeof v === "number" && v % 2 === 0;
  }
}
class ShortString {
  static [Symbol.hasInstance](v: any) {
    return typeof v === "string" && v.length < 6;
  }
}

const lhs: [string, any][] = [];
for (let n = 0; n <= 7; n++) lhs.push([`sso${n}`, rt("abcdefgh".slice(0, n))]);
lhs.push(["concat3", concat]);
lhs.push(["literal", "uri"]);
lhs.push(["heapString", rt("x".repeat(40))]);
lhs.push(["float", rt(1.5)]);
lhs.push(["zero", rt(0)]);
lhs.push(["negZero", -0]);
lhs.push(["nan", NaN]);
lhs.push(["denormal", rt(1e-310)]);
lhs.push(["int", rt(5) | 0]);
lhs.push(["even", rt(4)]);
lhs.push(["bigint", BigInt(rt(7))]);
lhs.push(["hugeBigint", 2n ** 70n]);
lhs.push(["symbol", Symbol("s")]);
lhs.push(["wellKnownSymbol", Symbol.iterator]);
lhs.push(["null", rt(null)]);
lhs.push(["undefined", undefined]);
lhs.push(["true", rt(true)]);
lhs.push(["false", false]);
lhs.push(["function", function g() {}]);
lhs.push(["arrow", () => 1]);
lhs.push(["array2", [1, 2]]);
lhs.push(["array3", rt([1, 2, 3])]);
lhs.push(["objectLiteral", { a: 1 }]);
lhs.push(["jsonObject", rt({ a: 1 })]);
lhs.push(["newA", new A()]);
lhs.push(["newB", new B()]);
lhs.push(["newF", new (F as any)()]);
lhs.push(["createA", Object.create(A.prototype)]);
lhs.push(["createB", Object.create(B.prototype)]);
lhs.push(["createObject", Object.create({})]);
lhs.push(["proxyA", new Proxy(new A(), {})]);
lhs.push(["proxyObject", new Proxy({}, {})]);
lhs.push(["date", new Date(0)]);
lhs.push(["map", new Map()]);
lhs.push(["regexp", /x/]);
lhs.push(["typeError", new TypeError("t")]);
lhs.push(["boxedString", new String("ab")]);
lhs.push(["boxedNumber", Object(3)]);

const pick = rt(1);
const rhs: [string, any][] = [
  ["A", A],
  ["B", B],
  ["F", F],
  ["Object", Object],
  ["Function", Function],
  ["Array", Array],
  ["String", String],
  ["Number", Number],
  ["Error", Error],
  ["TypeError", TypeError],
  ["Date", Date],
  ["Map", Map],
  ["Promise", Promise],
  ["Even", Even],
  ["ShortString", ShortString],
  ["dynamicA", pick ? A : B],
];

const cell = (f: () => boolean): string => {
  try {
    return f() ? "T" : "F";
  } catch (e: any) {
    return "E";
  }
};

console.log("LHS order:", lhs.map(([name]) => name).join(" "));
for (const [name, R] of rhs) {
  console.log(`dynamic ${name.padEnd(11)} ${lhs.map(([, v]) => cell(() => v instanceof R)).join("")}`);
}
// Static right-hand sides (compile-time class ids / builtin ids).
const statics: [string, (v: any) => boolean][] = [
  ["A", (v) => v instanceof A],
  ["B", (v) => v instanceof B],
  ["F", (v) => v instanceof F],
  ["K", (v) => v instanceof K],
  ["Object", (v) => v instanceof Object],
  ["Function", (v) => v instanceof Function],
  ["Array", (v) => v instanceof Array],
  ["Error", (v) => v instanceof Error],
  ["Date", (v) => v instanceof Date],
  ["Map", (v) => v instanceof Map],
  ["Promise", (v) => v instanceof Promise],
  ["Even", (v) => v instanceof Even],
];
for (const [name, test] of statics) {
  console.log(`static  ${name.padEnd(11)} ${lhs.map(([, v]) => cell(() => test(v))).join("")}`);
}

// --- reflective @@hasInstance -----------------------------------------------------
const hasInstance = (Function.prototype as any)[Symbol.hasInstance];
for (const [name, v] of lhs.slice(0, 11)) {
  console.log(`Function.prototype[@@hasInstance].call(K, ${name}):`, hasInstance.call(K, v));
}

// --- non-callable / primitive right-hand side ---------------------------------------
const badRhs: [string, any][] = [
  ["{}", {}],
  ["[]", []],
  ["5", rt(5)],
  ["sso", rt("ab")],
  ["null", rt(null)],
  ["undefined", undefined],
];
for (const [name, R] of badRhs) {
  for (const [lname, v] of [["sso3", rt("uri")], ["newA", new A()], ["null", null]] as [string, any][]) {
    try {
      console.log(`${lname} instanceof ${name}:`, (v as any) instanceof R);
    } catch (e: any) {
      console.log(`${lname} instanceof ${name}: ${e.constructor.name}: ${e.message}`);
    }
  }
}

// --- ajv shape: `arg instanceof _Code` over mixed code arguments --------------------
class _CodeOrName {}
class _Code extends _CodeOrName {
  _items: any[];
  constructor(items: any[]) {
    super();
    this._items = items;
  }
}
function addCodeArg(code: string[], arg: any): void {
  if (arg instanceof _Code) code.push(...arg._items);
  else if (arg instanceof _CodeOrName) code.push("<name>");
  else code.push(typeof arg === "string" ? JSON.stringify(arg) : String(arg));
}
const code: string[] = [];
for (const arg of rt(["uri", "a", "", "abcdef", "format", 1, null, true]) as any[]) addCodeArg(code, arg);
addCodeArg(code, new _Code(["x", "y"]));
addCodeArg(code, new _CodeOrName());
console.log("ajv addCodeArg:", code.join(" "));

// --- sibling predicates that took the same decode -------------------------------------
for (const [name, v] of lhs.slice(0, 9)) {
  console.log(
    `util.types ${name}:`,
    util.types.isMapIterator(v),
    util.types.isSetIterator(v),
    util.types.isPromise(v),
    util.types.isDate(v),
  );
}
console.log("util.types real iterators:", util.types.isMapIterator(new Map().keys()), util.types.isSetIterator(new Set().values()));

// --- hot path: the class-instance hit/miss answers stay intact ------------------------
let hits = 0;
let misses = 0;
const a = new A();
const bb = new B();
for (let i = 0; i < 1000; i++) {
  if (a instanceof A) hits++;
  if (bb instanceof A) hits++;
  if (!(a instanceof B)) misses++;
}
console.log("hot loop:", hits, misses);
