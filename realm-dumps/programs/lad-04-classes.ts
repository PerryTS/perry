abstract class Shape {
  static count = 0;
  name: string;
  constructor(name: string) { this.name = name; Shape.count++; }
  abstract area(): number;
  describe(): string { return `${this.name} with area ${this.area().toFixed(2)}`; }
}
class Circle extends Shape {
  r: number;
  constructor(r: number) { super("circle"); this.r = r; }
  area() { return Math.PI * this.r * this.r; }
}
class Square extends Shape {
  #side: number;
  constructor(side: number) { super("square"); this.#side = side; }
  area() { return this.#side ** 2; }
  get side() { return this.#side; }
  set side(v: number) { this.#side = v; }
  describe() { return "[" + super.describe() + "]"; }
}
const shapes: Shape[] = [new Circle(1), new Square(3)];
for (const s of shapes) console.log(s.describe());
const sq = shapes[1] as Square;
sq.side = 4;
console.log(sq.side, sq.area(), Shape.count, sq instanceof Shape, sq instanceof Circle);
console.log(Object.getPrototypeOf(sq) === Square.prototype, sq.constructor.name);
