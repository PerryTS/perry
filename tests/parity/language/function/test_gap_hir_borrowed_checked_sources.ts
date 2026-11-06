const max = Math.max;
const parse = JSON.parse;
const fromCharCode = String.fromCharCode;
const resolve = Promise.resolve;
const slice = Array.prototype.slice;
const toString = Object.prototype.toString;
const hasOwnProperty = Object.prototype.hasOwnProperty;
const source = process.argv[2] ||
  "Math.max = function () { return 801; }; JSON.parse = function () { return 802; };" +
  "String.fromCharCode = function () { return 803; }; Promise.resolve = function () { return 804; };" +
  "Array.prototype.slice = function () { return 805; };" +
  "Object.prototype.toString = function () { return 806; };" +
  "Object.prototype.hasOwnProperty = function () { return 807; };";
new Function(source)();
const a = Math.max.call(null, 2, 3);
const b = JSON.parse.apply(null, ["{}"]);
const c = String.fromCharCode.call(null, 65);
const d = Promise.resolve.call(Promise, 1);
const e = Array.prototype.slice.call([1, 2], 0);
const f = Object.prototype.toString.call({});
const g = Object.hasOwnProperty.call({ x: 1 }, "x");
const restore = process.argv[3] ||
  "Math.max = max; JSON.parse = parse; String.fromCharCode = fromCharCode;" +
  "Promise.resolve = resolve; Array.prototype.slice = slice;" +
  "Object.prototype.toString = toString; Object.prototype.hasOwnProperty = hasOwnProperty;";
new Function("max", "parse", "fromCharCode", "resolve", "slice", "toString", "hasOwnProperty", restore)
  (max, parse, fromCharCode, resolve, slice, toString, hasOwnProperty);
console.log(a, b, c, d, e, f, g);
