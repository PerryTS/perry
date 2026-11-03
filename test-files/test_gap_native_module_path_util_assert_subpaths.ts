// req() of path/posix, path/win32, util/types and assert/strict: the subpath
// modules are the same objects as path.posix, path.win32, util.types and
// assert.strict, and every export must run the real implementation.
import { createRequire } from "node:module";
const req = createRequire(import.meta.url);
"use strict";
const path = req("path");
const util = req("util");
const assert = req("assert");
const posix = req("path/posix");
const posixPrefixed = req("node:path/posix");
const win32 = req("path/win32");
const win32Prefixed = req("node:path/win32");
const { join: posixJoin, basename } = req("path/posix");
const { join: winJoin } = req("path/win32");
const types = req("util/types");
const typesPrefixed = req("node:util/types");
const { isPromise, isDate } = req("util/types");
const strict = req("assert/strict");
const strictPrefixed = req("node:assert/strict");
const { strictEqual, throws } = req("assert/strict");
const attempt = (fn) => { try { return fn(); } catch (e) { return "threw " + e.name + " " + e.code; } };

console.log("path/posix", posix === path.posix, posixPrefixed === posix, path.posix.posix === path.posix);
console.log("path/win32", win32 === path.win32, win32Prefixed === win32);
for (const n of ["join", "resolve", "basename", "dirname", "extname", "parse", "format", "normalize",
  "relative", "isAbsolute", "toNamespacedPath"]) {
  console.log("path", n, typeof posix[n], typeof win32[n], posix[n] === path.posix[n], win32Prefixed[n] === path.win32[n]);
}
console.log("path sep", posix.sep, JSON.stringify(win32.sep), posix.delimiter, win32.delimiter);
console.log("posix join", posix.join("a", "b", "..", "c"), posixPrefixed.join("/x", "y"), (0, posix.join)("p", "q"), posixJoin("m", "n"));
console.log("win32 join", win32.join("a", "b", "..", "c"), win32Prefixed.join("C:\\x", "y"), (0, win32.join)("p", "q"), winJoin("m", "n"));
console.log("basename", posix.basename("/a/b.txt", ".txt"), win32.basename("C:\\a\\b.txt"), basename("/z/y"));
console.log("dirname", posix.dirname("/a/b/c"), win32.dirname("C:\\a\\b\\c"));
console.log("extname", posix.extname("f.tar.gz"), win32.extname("C:\\f.tar.gz"));
console.log("isAbsolute", posix.isAbsolute("/a"), win32.isAbsolute("C:\\a"), posix.isAbsolute("a"), win32.isAbsolute("a"));
console.log("parse", JSON.stringify(posix.parse("/home/u/f.txt")), JSON.stringify(win32.parse("C:\\u\\f.txt")));
console.log("format", posix.format({ dir: "/d", base: "f.txt" }), win32.format({ dir: "C:\\d", base: "f.txt" }));
console.log("normalize", posix.normalize("/a//b/../c"), win32.normalize("C:\\a\\\\b\\..\\c"));
console.log("relative", posix.relative("/a/b", "/a/c/d"), win32.relative("C:\\a\\b", "C:\\a\\c\\d"));
console.log("resolve", posix.resolve("/a", "b", "../c"), win32.resolve("C:\\a", "b", "..\\c"));

console.log("util/types", types === util.types, typesPrefixed === types);
for (const n of ["isPromise", "isDate", "isRegExp", "isMap", "isSet", "isTypedArray", "isUint8Array",
  "isArrayBuffer", "isAnyArrayBuffer", "isNativeError", "isAsyncFunction", "isGeneratorFunction",
  "isProxy", "isBoxedPrimitive", "isWeakMap", "isSymbolObject"]) {
  console.log("util/types", n, typeof types[n], typeof typesPrefixed[n], types[n] === util.types[n]);
}
console.log("isPromise", types.isPromise(Promise.resolve()), types.isPromise({}), (0, types.isPromise)(Promise.resolve()), isPromise(1));
console.log("isDate", types.isDate(new Date()), typesPrefixed.isDate(Date.now()), isDate(new Date(0)));
console.log("isRegExp", types.isRegExp(/a/), types.isRegExp("a"));
console.log("isMap/isSet", types.isMap(new Map()), types.isSet(new Set()), types.isMap(new Set()), types.isSet(new Map()));
console.log("isTypedArray", types.isTypedArray(new Uint8Array(1)), types.isUint8Array(new Uint8Array(1)), types.isUint8Array(new Int8Array(1)), types.isTypedArray([]));
console.log("isArrayBuffer", types.isArrayBuffer(new ArrayBuffer(1)), types.isAnyArrayBuffer(new ArrayBuffer(1)), types.isArrayBuffer({}));
console.log("isNativeError", types.isNativeError(new TypeError("x")), types.isNativeError({ message: "x" }));
console.log("isAsyncFunction", types.isAsyncFunction(async () => {}), types.isAsyncFunction(() => {}), types.isGeneratorFunction(function* () {}));
console.log("isProxy", types.isProxy(new Proxy({}, {})), types.isProxy({}));
console.log("isBoxedPrimitive", types.isBoxedPrimitive(Object(1)), types.isBoxedPrimitive(1), types.isWeakMap(new WeakMap()), types.isSymbolObject(Object(Symbol())));

console.log("assert/strict", strict === assert.strict, strictPrefixed === strict);
for (const n of ["ok", "equal", "notEqual", "deepEqual", "notDeepEqual", "strictEqual", "notStrictEqual",
  "deepStrictEqual", "throws", "doesNotThrow", "rejects", "fail", "match", "ifError"]) {
  console.log("assert/strict", n, typeof strict[n], typeof strictPrefixed[n], strict[n] === assert.strict[n]);
}
console.log("strictEqual", attempt(() => strict.strictEqual(1, 1)), attempt(() => strict.strictEqual(1, 2)), attempt(() => strictEqual(1, "1")));
console.log("equal is strict", attempt(() => strict.equal(1, "1")), attempt(() => strict.equal(2, 2)));
console.log("deepEqual is strict", attempt(() => strict.deepEqual({ a: 1 }, { a: "1" })), attempt(() => strict.deepEqual({ a: 1 }, { a: 1 })));
console.log("deepStrictEqual", attempt(() => strictPrefixed.deepStrictEqual([1, { b: 2 }], [1, { b: 2 }])), attempt(() => (0, strict.deepStrictEqual)([1], [2])));
console.log("ok", attempt(() => strict.ok(true)), attempt(() => strict.ok(0)));
console.log("throws", attempt(() => strict.throws(() => { throw new Error("x"); })), attempt(() => strict.throws(() => {})), attempt(() => throws(() => { throw new TypeError("t"); }, TypeError)));
console.log("doesNotThrow", attempt(() => strict.doesNotThrow(() => {})), attempt(() => strict.doesNotThrow(() => { throw new Error("y"); })));
console.log("fail", attempt(() => strict.fail("boom")));
console.log("notStrictEqual", attempt(() => strict.notStrictEqual(1, 2)), attempt(() => strict.notStrictEqual(1, 1)));
console.log("match", attempt(() => strict.match("abc", /b/)), attempt(() => strict.match("abc", /z/)));
(async () => {
  console.log("rejects", await strict.rejects(Promise.reject(new Error("r"))).then(() => "ok", (e) => "threw " + e.name));
  console.log("rejects fails", await strict.rejects(Promise.resolve(1)).then(() => "ok", (e) => "threw " + e.name + " " + e.code));
  process.exit(0);
})();
