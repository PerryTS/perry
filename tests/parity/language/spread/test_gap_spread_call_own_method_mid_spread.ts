// The method is selected before the arguments: an iterator that adds an own `set` to the
// receiver, or rewrites the prototype method, still calls the method read first.
class V { set(a, b, c) { return "proto:" + a + b + c; } }
const v = new V();
class G { constructor(f) { this.f = f; } *[Symbol.iterator]() { this.f(); yield 1; yield 2; } }
console.log(v.set(...new G(() => { v.set = function () { return "own"; }; }), 3));
console.log(v.set(...[4, 5], 6));
delete v.set;
console.log(v.set(0, ...new G(() => { V.prototype.set = function () { return "rewritten"; }; })));
console.log(v.set(...[7, 8, 9]));
