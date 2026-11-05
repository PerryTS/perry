// #10483: `Math[k]` / `JSON[k]` / `Object[k]` / ... with a *variable* (non-literal)
// computed key must resolve against the real namespace/constructor object, not the
// number 0. #973 rerouted bare builtin idents used as VALUES to
// `PropertyGet{GlobalGet(0), name}`; member_tail.rs undoes that reroute in
// member-OBJECT position so the intrinsic call / constant-fold paths for a
// STATICALLY-KNOWN member name (`Math.max(...)`) keep their pre-#973 `GlobalGet(0)`
// receiver. That undo is wrong for a computed non-literal key — nothing can resolve
// to an intrinsic at lowering time, so the receiver collapsed to the bare `GlobalGet(0)`
// sentinel and the read landed on the number 0. Closed #6677 fixed only the
// string-literal direct-call form (`Math["max"](...)`); this covers the variable-key
// forms it left broken, in both call position and value-read position, with keys from
// a plain variable, a template literal, and a function return. Static (literal) key
// paths are re-asserted unchanged at the end — that's the entire point of the
// reroute-undo this fix narrows (test_gap_number_math regressed when #973 first
// landed).

// ---------------------------------------------------------------------------
// call position, key from a plain variable
// ---------------------------------------------------------------------------
function callMath(key: string, ...args: number[]): number {
  // @ts-ignore -- deliberately untyped computed access, the #10483 repro shape
  return Math[key](...args);
}
console.log("call/variable:");
console.log(callMath("max", 1, 5, 3));
console.log(callMath("min", 1, 5, 3));
console.log(callMath("round", 2.6));

function callJson(key: string, value: unknown): string {
  // @ts-ignore
  return JSON[key](value);
}
console.log(callJson("stringify", { a: 1, b: [1, 2, 3] }));

function callReflect(key: string, target: object, prop: string): boolean {
  // @ts-ignore
  return Reflect[key](target, prop);
}
console.log(callReflect("has", { x: 1 }, "x"));
console.log(callReflect("has", { x: 1 }, "y"));

function callNumber(key: string, value: number): boolean {
  // @ts-ignore
  return Number[key](value);
}
console.log(callNumber("isInteger", 5));
console.log(callNumber("isInteger", 5.5));

function callArray(key: string, value: unknown): boolean {
  // @ts-ignore
  return Array[key](value);
}
console.log(callArray("isArray", [1, 2]));
console.log(callArray("isArray", {}));

function callString(key: string, ...codes: number[]): string {
  // @ts-ignore
  return String[key](...codes);
}
console.log(callString("fromCharCode", 72, 105));

function callDate(key: string): boolean {
  // @ts-ignore
  return typeof Date[key]() === "number";
}
console.log(callDate("now"));

// ---------------------------------------------------------------------------
// value-read position (not called), key from a plain variable
// ---------------------------------------------------------------------------
console.log("value-read/variable:");
function readTypeof(obj: unknown, key: string): string {
  // @ts-ignore
  return typeof obj[key];
}
console.log(readTypeof(Math, "max"));
console.log(readTypeof(JSON, "stringify"));
console.log(readTypeof(Reflect, "ownKeys"));
console.log(readTypeof(Number, "isInteger"));
console.log(readTypeof(Date, "now"));
console.log(readTypeof(Array, "isArray"));
console.log(readTypeof(String, "fromCharCode"));

function grabMax(key: string): (...xs: number[]) => number {
  // @ts-ignore
  return Math[key];
}
const maxFn = grabMax("max");
console.log(typeof maxFn, maxFn(7, 42, 3));

// ---------------------------------------------------------------------------
// key from a template literal
// ---------------------------------------------------------------------------
console.log("template-literal key:");
function templateCall(part: string): number {
  // @ts-ignore
  return Math[`${part}`](4, 9);
}
console.log(templateCall("max"));
console.log(templateCall("min"));

function templateRead(part: string): string {
  // @ts-ignore
  return typeof JSON[`${part}`];
}
console.log(templateRead("parse"));

// ---------------------------------------------------------------------------
// key from a function return
// ---------------------------------------------------------------------------
console.log("function-return key:");
function pickMaxKey(): string {
  return "max";
}
// @ts-ignore
console.log(Math[pickMaxKey()](4, 9));

function pickIsIntegerKey(): string {
  return "isInteger";
}
// @ts-ignore
console.log(Number[pickIsIntegerKey()](10));

// ---------------------------------------------------------------------------
// static (literal) key controls — must stay byte-identical to before #10483
// ---------------------------------------------------------------------------
console.log("static controls:");
console.log(Math.max(1, 5, 3));
console.log(Math["max"](1, 5, 3));
console.log(Math.min(1, 5, 3));
console.log(JSON.stringify({ z: 1 }));
console.log(JSON["stringify"]({ z: 1 }));
console.log(Number.parseInt("42", 10));
console.log(Number.isInteger(5));
console.log(Array.isArray([1]));
console.log(Reflect.has({ x: 1 }, "x"));

// ---------------------------------------------------------------------------
// `console`'s pre-existing dynamic dispatch (#js_console_method_by_value,
// the Next.js `prefixedLog` fix) must keep working — #10483's new guard
// deliberately excludes "console" so the call-position undo it depends on
// still fires.
// ---------------------------------------------------------------------------
console.log("console dynamic dispatch:");
function consoleCall(method: string, msg: string): void {
  // @ts-ignore
  console[method](msg);
}
consoleCall("log", "console[method](...) still dispatches");

function consoleValueRead(method: string): string {
  // @ts-ignore
  return typeof console[method];
}
console.log(consoleValueRead("log"));
