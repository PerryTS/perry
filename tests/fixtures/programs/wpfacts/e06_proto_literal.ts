// E6 (literal form): a literal born with a class prototype is its own
// template; the method's receiver set is no longer the class alone, and
// `k` sits at a different slot in it.
// WPFACTS: site .k off
class K {
  k: number = 5;
  get2() { return this.k * 2; }
}
const fake: any = { __proto__: K.prototype, pad: 0, k: "s" };
function rd(x: K) { return x.k; }
console.log(rd(new K()), rd(fake), fake.get2(), new K().get2());
