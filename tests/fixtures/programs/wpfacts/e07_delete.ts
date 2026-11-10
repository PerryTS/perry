// E7: delete removes a birth key; a prototype getter then answers.
// WPFACTS: fact F3 prefix_stable Del = 0
// WPFACTS: site .f off
class Del {
  f: number = 1;
  g: number = 2;
}
Object.defineProperty(Del.prototype, "f", { get() { return 100; }, configurable: true });
const d = new Del();
delete (d as any).f;
function rd(x: Del) { return x.f; }
console.log(rd(d), d.g);
