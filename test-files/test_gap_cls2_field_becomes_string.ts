// D2 negative: a field that becomes a string after construction.
class Shape {
  x: number; y: number;
  constructor(x: number, y: number) { this.x = x; this.y = y; }
  area(): number { return 2; }
  describe(): number { return this.area() + this.x + this.y; }
  scaled(): number { return this.x * this.y; }
}
class Rect extends Shape {
  w: number; h: number;
  constructor(x: number, y: number, w: number, h: number) { super(x, y); this.w = w; this.h = h; }
  area(): number { return this.w * this.h; }
}
const a: Shape = new Rect(1, 2, 3, 4);
const b: Shape = new Shape(5, 6);
const log: any[] = [];
log.push(a.describe(), b.describe(), a.scaled());
(a as any).x = "x";
(b as any).y = "y";
log.push(a.describe(), b.describe(), a.scaled());
(a as any).w = "w";
log.push((a as any).area(), a.describe());
console.log(log.map(String).join("|"));
