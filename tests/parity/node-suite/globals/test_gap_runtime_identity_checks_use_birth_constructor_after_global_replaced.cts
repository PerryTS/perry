// The runtime's identity checks against a builtin constructor compare with the
// realm's original one: `assert.throws(fn, X)` and the ERR_INVALID_ARG_TYPE
// "Received" text of a typed array (or other native object) whose
// `constructor` names nothing. Replacing the global binding first
// (`globalThis.TypeError = X`) changes neither: node never reads the global.
const g: any = globalThis;
const errors = ["Error", "TypeError", "RangeError", "ReferenceError", "SyntaxError", "EvalError", "URIError", "AggregateError"];
const natives = [
  "ArrayBuffer", "SharedArrayBuffer", "DataView", "Date", "Uint8Array", "Int8Array", "Uint16Array",
  "Int16Array", "Uint32Array", "Int32Array", "Float32Array", "Float64Array", "Uint8ClampedArray",
  "BigInt64Array", "BigUint64Array",
];
const saved: any = {};
for (const n of [...errors, ...natives]) saved[n] = g[n];
const assert = require("assert");
const util = require("util");
const defineProperty = Object.defineProperty;
const replaced: any = {};
for (const n of [...errors, ...natives]) {
  replaced[n] = class Replaced {};
  g[n] = replaced[n];
}

for (const n of errors) {
  const thrown = n === "AggregateError" ? new saved[n]([], "x") : new saved[n]("x");
  let out = "passed";
  try {
    assert.throws(() => { throw thrown; }, replaced[n]);
  } catch (e: any) {
    out = e === thrown ? "rethrew" : "threw other";
  }
  console.log("assert.throws", n, out);
}
function make(n: string): any {
  if (n === "DataView") return new saved.DataView(new saved.ArrayBuffer(0));
  if (n === "Date") return new saved.Date(0);
  return new saved[n](0);
}
for (const n of natives) {
  const value = make(n);
  defineProperty(value, "constructor", { value: undefined });
  try {
    util.promisify(value);
  } catch (e: any) {
    console.log("received", n, e.message.slice(e.message.indexOf("Received")));
  }
}
