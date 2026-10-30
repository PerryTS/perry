// Classes whose layout the facts prove fixed: the template method's dispatch
// on `this` calls the proven-receiver body directly, with no layout recheck.
// The semantics must be exactly the dispatch's, overrides and inherited
// fields included.
class Shape {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
  area(): number {
    return 0;
  }
  describe(): number {
    return this.area() + this.x + this.y;
  }
}

class Circle extends Shape {
  r: number;
  constructor(x: number, y: number, r: number) {
    super(x, y);
    this.r = r;
  }
  area(): number {
    return 3 * this.r * this.r;
  }
}

class Rect extends Shape {
  w: number;
  h: number;
  constructor(x: number, y: number, w: number, h: number) {
    super(x, y);
    this.w = w;
    this.h = h;
  }
  area(): number {
    return this.w * this.h;
  }
}

class Tri extends Shape {
  b: number;
  h: number;
  constructor(x: number, y: number, b: number, h: number) {
    super(x, y);
    this.b = b;
    this.h = h;
  }
  area(): number {
    return (this.b * this.h) / 2;
  }
}

const shapes: Shape[] = [];
for (let i = 0; i < 12; i++) {
  if (i % 3 === 0) shapes.push(new Circle(i, i + 1, (i % 4) + 1));
  else if (i % 3 === 1) shapes.push(new Rect(i, 2, (i % 5) + 1, 2));
  else shapes.push(new Tri(i, 1, (i % 3) + 2, 4));
}
let total = 0;
for (let r = 0; r < 3; r++) {
  for (let i = 0; i < shapes.length; i++) {
    const s = shapes[i];
    s.x = (s.x + 1) % 10;
    total += s.describe();
  }
}
console.log(total);
console.log(shapes.map((s) => s.describe()).join(","));
