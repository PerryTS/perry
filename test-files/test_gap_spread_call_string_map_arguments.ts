// Spread of a string (code points), a Map (entries), a Set, and an arguments object, into a method and a function.
class V { set(a, b, c, d) { return JSON.stringify([a, b, c, d]); } }
function f(a, b, c, d) { return JSON.stringify([a, b, c, d]); }
const v = new V();
console.log(v.set(..."a\u{1F600}b"), f(..."hey", 1));
const m = new Map([[1, "a"], [2, "b"]]);
console.log(v.set(...m), f(...m, 0), v.set(...new Set([5, 6, 5])));
function viaArgs() { return v.set(...arguments, "z"); }
function viaArgs2() { return f(9, ...arguments); }
console.log(viaArgs(1, 2), viaArgs2(3, 4));
