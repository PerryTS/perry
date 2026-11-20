// GetIterator does GetMethod(@@iterator) once: a getter on it runs once per spread.
let gets = 0;
class It { get [Symbol.iterator]() { gets++; return function* () { yield 1; yield 2; }; } }
function f(a, b, c) { return [a, b, c].join(","); }
const o = new It();
console.log(f(...o, 3), gets);
console.log(Math.max(...o), gets);
console.log([...o].length, gets);
class R { set(a, b, c) { return a + b + c; } }
console.log(new R().set(...o, 10), gets);
