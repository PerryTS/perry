// E9: the instance is a Proxy target; the proxy reaches a method.
// WPFACTS: fact F3 prefix_stable Tgt = 0
class Tgt {
  q: number = 1;
  bump() { this.q = this.q + 1; return this.q; }
}
const t = new Tgt();
const p = new Proxy(t, {
  set(o: any, k: string, v: any) { o[k] = v * 10; return true; },
});
console.log(Tgt.prototype.bump.call(p), t.q);
