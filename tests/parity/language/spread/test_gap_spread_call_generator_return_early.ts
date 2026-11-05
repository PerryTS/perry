// A generator that returns early: fewer args (rest undefined), finally blocks run, `return` value not spread.
let fin = 0;
class G { constructor(n) { this.n = n; } *[Symbol.iterator]() { try { yield 1; if (this.n < 2) return 99; yield 2; yield 3; } finally { fin++; } } }
class V { set(a, b, c, d) { return [a, b, c, d].map(String).join(","); } }
function f(...a) { return a.length + ":" + a.join(","); }
console.log(new V().set(...new G(1), 7), f(...new G(1)), f(...new G(5), 7), fin);
