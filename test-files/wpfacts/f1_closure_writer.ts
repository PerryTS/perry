// F1: a closure captures the instance and writes the field later.
// WPFACTS: fact F1 readonly Cnt.c = 0
class Cnt {
  c: number = 0;
}
const k = new Cnt();
const inc = () => { k.c = k.c + 1; };
inc();
inc();
console.log(k.c);
