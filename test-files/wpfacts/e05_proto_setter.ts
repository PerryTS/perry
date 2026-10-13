// E5/E23: an accessor on Object.prototype named after a birth key.
// WPFACTS: fact F3 prefix_stable Plain = 0
class Plain {
  w: number = 3;
}
Object.defineProperty(Object.prototype, "zz", {
  set(v: number) { console.log("setter", v); },
  get() { return 9; },
  configurable: true,
});
const p = new Plain();
console.log(p.w, (p as any).zz);
