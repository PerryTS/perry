// E1: an `any` alias writes a string into a number field.
// WPFACTS: fact F1 readonly Box.v = 0
// WPFACTS: kind Box.v has str
class Box {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
}
const b = new Box(1);
const a: any = b;
a.v = "x";
console.log(typeof b.v, b.v);
