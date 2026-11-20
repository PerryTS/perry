// E24: sloppy mapped `arguments` aliases a parameter. A `.cjs` file: the
// repo's package.json makes `.js` an ES module, which is strict and unmapped.
// The write through `arguments[0]` reaches `a`: the site's receiver set must
// hold both P1 and P2 (a proven two-shape set is sound; P1 alone is not).
// WPFACTS: recv .p P1 P2
class P1 { constructor() { this.p = 1; } }
class P2 { constructor() { this.p = "two"; } }
function rd(a) {
  arguments[0] = new P2();
  return a.p;
}
console.log(rd(new P1()));
