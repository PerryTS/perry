// #11335: a store `arr[k] = v` whose key is only known at run time, into an
// array held by a BOXED local (a `var` that a closure captures), must publish
// the grown array through the box. Perry stored the reallocated head straight
// into the local's stack slot — which, for a boxed local, holds the box
// pointer — so the first store that grew the array past its capacity clobbered
// the box, and every later read of the binding returned `undefined` or crashed.
// iconv-lite's utf7 codec builds its tables this way at load time, so every
// compiled mysql2 program segfaulted inside `createConnection`.
import table from "./fixtures/issue_11335/table.cjs";

// A `var` in a function expression, captured by an inner closure.
const fnExpr = function () {
  var arr = [];
  for (var i = 0; i < 40; i++) { arr[i] = i * 2; }
  var at = function (n) { return arr[n]; };
  return arr.length + " " + at(39) + " " + at(3);
};
console.log("function expression:", fnExpr());

// The same with the growth done by the closure itself.
const grownInClosure = function () {
  var arr = [];
  var grow = function (n) { for (var k = 0; k < n; k++) { arr[k] = "v" + k; } };
  grow(100);
  return arr.length + " " + arr[99];
};
console.log("grown in closure:", grownInClosure());

// A key that is not an array index still names a property (control for the
// runtime-key helper's other arm).
const oddKeys = function () {
  var arr = [];
  var keys = [0, 1, 2, "x", 1.5, -1, 3];
  for (var j = 0; j < keys.length; j++) { arr[keys[j]] = j; }
  var read = function () { return JSON.stringify(Object.keys(arr)); };
  return arr.length + " " + read();
};
console.log("odd keys:", oddKeys());

// The CommonJS shape from iconv-lite.
console.log("cjs table:", table.count, table.decoder.isBase64(65), table.decoder.isBase64(44), table.imap.isBase64(44));
