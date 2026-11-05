// f(...a, x, ...g), a spread in the middle, and m(...v3) into a rest method.
function f(a, b, c, d, e) { return [a, b, c, d, e].map(String).join(","); }
function* g() { yield "g1"; yield "g2"; }
class Vec3 { constructor() { this.x = 1; this.y = 2; this.z = 3; } *[Symbol.iterator]() { yield this.x; yield this.y; yield this.z; } }
class M { fixed(a, b, c, d) { return [a, b, c, d].map(String).join(","); } rest(first, ...more) { return first + "|" + more.length + "|" + more.join(","); } args() { return arguments.length + ":" + Array.prototype.join.call(arguments, ","); } }
const m = new M(), a = [1, 2], v3 = new Vec3();
let side = 0;
console.log(f(...a, "x", ...g()), m.fixed(...a, "x", ...g()));
console.log(m.fixed("p", ...a, (side++, "q")), side);
console.log(m.rest(...v3), m.rest(0, ...v3, 4), m.args(...v3, ...a));
console.log(m.fixed(...[]), m.fixed(...[1, 2, 3, 4, 5, 6]), m.rest(...[]));
const big = Array.from({ length: 40 }, (_, i) => i);
console.log(m.fixed(...big), m.rest(...big).length);
