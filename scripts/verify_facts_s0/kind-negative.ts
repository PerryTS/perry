class Box {
  v: any;
  constructor(v: any) { this.v = v; }
}
const b = new Box(1);
const a: any = b;
a.v = "x";
console.log(typeof b.v, b.v);
