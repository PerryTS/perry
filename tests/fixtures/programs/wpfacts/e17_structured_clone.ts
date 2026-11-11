// E17: a structured clone does not keep class identity.
// WPFACTS: site .s off
class Cl {
  s: number = 4;
}
const c = new Cl();
const copy = structuredClone(c) as any as Cl;
let total = 0;
// One read site sees both the instance and the clone (no per-call copy).
for (const x of [c, copy]) total += x.s;
console.log(total, copy instanceof Cl);
