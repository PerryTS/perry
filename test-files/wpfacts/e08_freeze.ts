// E8: a frozen instance; a later store must throw (strict).
// WPFACTS: fact F3 prefix_stable Frz = 0
class Frz {
  a: number = 1;
  set(v: number) { this.a = v; }
}
const f = new Frz();
Object.freeze(f);
try {
  f.set(2);
  console.log("no throw", f.a);
} catch (e) {
  console.log("threw", f.a);
}
