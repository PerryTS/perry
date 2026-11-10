// A patched Array.prototype[Symbol.iterator] is observed by call spread.
function f(a, b, c, d) { return [a, b, c, d].join(","); }
class V { set(a, b, c, d) { return [a, b, c, d].join(","); } }
const arr = [1, 2, 3];
console.log(f(...arr, 9), new V().set(...arr, 9));
const orig = Array.prototype[Symbol.iterator];
Array.prototype[Symbol.iterator] = function* () { for (const x of orig.call(this)) yield x * 10; };
console.log(f(...arr, 9), new V().set(...arr, 9), Math.max(...arr));
Array.prototype[Symbol.iterator] = orig;
console.log(f(...arr, 9), new V().set(...arr, 9));
