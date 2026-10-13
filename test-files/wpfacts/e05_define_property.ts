// E5: defineProperty turns a birth key into an accessor.
// WPFACTS: fact F3 prefix_stable Acc = 0
// WPFACTS: site .v off
class Acc {
  v: number = 1;
}
const o = new Acc();
Object.defineProperty(o, "v", { get() { return 42; } });
function rd(a: Acc) { return a.v; }
console.log(rd(o), rd(new Acc()));
