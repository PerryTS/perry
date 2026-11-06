// The method is read before the arguments: an iterator that replaces the method still calls the old one.
class V { set(a, b) { return "old:" + a + b; } }
class G { *[Symbol.iterator]() { V.prototype.set = function (a, b) { return "new:" + a + b; }; yield 1; yield 2; } }
const v = new V();
console.log(v.set(...new G()));
console.log(v.set(...[3, 4]));
