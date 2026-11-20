class Base { constructor(o) { return o; } }
class D extends Base {
  #f = 21;
  get #a() { return this.#f * 2; }
  static read(o) { return o.#a; }
}
function fn() {}
new D(fn);
console.log(D.read(fn));
