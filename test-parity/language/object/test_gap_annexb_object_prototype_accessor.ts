// `Object.prototype.__defineSetter__` / `__defineGetter__` install on
// Object.prototype, as defineProperty does: a store or read on a literal
// under that key runs the accessor with the literal as `this`.
class V {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
  twice(): number {
    return this.x * 2;
  }
}
let seen: any = null;
(Object.prototype as any).__defineSetter__("constructor", function (this: any, v: any) {
  seen = this;
  this.a = "s" + v;
});
const o: any = { a: new V(3) };
o.constructor = 5;
console.log(typeof o.a, String(o.a), seen === o);

(Object.prototype as any).__defineGetter__("constructor", function (this: any) {
  this.b = "read";
  return 1;
});
const p: any = { b: new V(4) };
const c = p.constructor;
console.log(c, typeof p.b, String(p.b));
const q: any = { b: new V(5) };
console.log(q.b.twice());
