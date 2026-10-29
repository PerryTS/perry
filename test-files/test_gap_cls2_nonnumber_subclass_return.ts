// D2 negative: a subclass override returns a non-Number; the closed-site
// Number proof must not cover it (describe() is then string concatenation).
class Shape {
  x: number; y: number;
  constructor(x: number, y: number) { this.x = x; this.y = y; }
  area(): number { return 0; }
  describe(): number { return this.area() + this.x + this.y; }
}
class Circle extends Shape {
  r: number;
  constructor(x: number, y: number, r: number) { super(x, y); this.r = r; }
  area(): number { return Math.PI * this.r * this.r; }
}
class Odd extends Shape {
  area(): number { return "7" as any; }
}
const shapes: Shape[] = [new Circle(1, 2, 3), new Odd(4, 5), new Shape(6, 7), new Odd(8, 9)];
let out: any[] = [];
for (let r = 0; r < 3; r++) for (let i = 0; i < shapes.length; i++) out.push(shapes[i].describe());
console.log(out.join("|"));
let total: any = 0;
for (let i = 0; i < shapes.length; i++) total += shapes[i].describe();
console.log(typeof total, String(total));
