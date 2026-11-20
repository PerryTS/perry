class Base { constructor(o) { return o; } }
class D extends Base { #f = 21; static read(o) { return o.#f; } }
function fn() {}
new D(fn);
console.log(D.read(fn));
