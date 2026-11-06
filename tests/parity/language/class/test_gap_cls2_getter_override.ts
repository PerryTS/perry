// D2 negative: a getter and an accessor-returned method shadow the data a
// proof would read.
class Shape {
  x: number; y: number;
  constructor(x: number, y: number) { this.x = x; this.y = y; }
  area(): number { return 1; }
  describe(): number { return this.area() + this.x + this.y; }
}
class Getter extends Shape {
  get area(): any { return () => "g"; }
}
class Plain extends Shape {}
const list: Shape[] = [new Plain(1, 2), new Getter(3, 4), new Plain(5, 6)];
const out: any[] = [];
for (let r = 0; r < 2; r++) for (const s of list) out.push(s.describe());
console.log(out.join("|"));
let t: any = 0;
for (const s of list) t += s.describe();
console.log(String(t));
