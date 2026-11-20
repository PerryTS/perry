// Issue #1777: Function.prototype.call/.apply on builtin prototype methods.
// `Array.prototype.slice.call(arguments, 1)` / `[].slice.call(arguments)` and
// the same on String/Object prototypes must dispatch to the native impl (the
// method value previously read `undefined` and `.call`/`.apply` threw).

// typeof of a borrowed prototype method is "function" (feature detection).
console.log(typeof Array.prototype.slice);
console.log(typeof Array.prototype.map);
console.log(typeof [].slice);
console.log(typeof String.prototype.charAt);
console.log(typeof "".slice);

// Array.prototype borrowing on a real array.
console.log(Array.prototype.slice.call([9, 8, 7], 1));
console.log(Array.prototype.map.call([1, 2, 3], (x: number) => x * 2));
console.log([].slice.call([5, 6, 7], 1));
console.log([].concat.call([1], [2, 3]));

// .apply with a clean literal args array.
console.log(Array.prototype.slice.apply([1, 2, 3, 4], [1, 3]));

// The classic arguments-to-array idiom. The borrowed Array method must accept
// real ECMAScript Arguments objects.
function args2arr() {
  return [].slice.call(arguments);
}
console.log(args2arr(1, 2, 3));

function tailArgs() {
  return Array.prototype.slice.call(arguments, 1);
}
console.log(tailArgs("a", "b", "c"));

// String.prototype borrowing.
console.log("hello".slice.call("world", 1));
console.log(String.prototype.toUpperCase.call("abc"));

// Object.prototype.{toString,hasOwnProperty}.call must still route through the
// existing runtime-helper rewrites (not regressed by the new hook).
const o = { a: 1 };
console.log(Object.prototype.hasOwnProperty.call(o, "a"));
console.log(Object.prototype.hasOwnProperty.call(o, "b"));
console.log(Object.prototype.toString.call([]));
console.log(Object.prototype.toString.call({}));
