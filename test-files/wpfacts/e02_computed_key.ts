// E2: a computed-key store with a key read from input.
// WPFACTS: fact F1 readonly Rec.f = 0
// WPFACTS: fact F3 fixed_layout Rec = 0
class Rec {
  f: number = 1;
  g: number = 2;
}
const r = new Rec();
const keys = process.argv.length > 99 ? ["g"] : ["f", "h"];
for (const k of keys) (r as any)[k] = 7;
console.log(r.f, r.g, (r as any).h);
