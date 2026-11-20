// E14: `new Function` with a constant body writes a field.
// WPFACTS: fact F1 readonly Dyn.f = 0
// WPFACTS: kind Dyn.f has str
class Dyn {
  f: number = 1;
}
const d = new Dyn();
const setter = new Function("o", "o.f = 'x';");
setter(d);
console.log(d.f);
