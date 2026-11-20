// CLS D1: the annotation says Shape, the value is another thing. Only proofs
// from the values may pick the class-id switch.
class Shape {
  x: number = 1;
  area(): number { return 0; }
  describe(): number { return this.area() + this.x; }
}
class Circle extends Shape {
  area(): number { return 3; }
}
class Rect extends Shape {
  area(): number { return 4; }
}

class Unrelated { x = 9; area(): number { return 1000; } describe(): number { return 2000; } }
const fake = { x: 5, area() { return 7; }, describe() { return 70; } } as any as Shape;
const other = new Unrelated() as any as Shape;
const shapes: Shape[] = [new Circle(), fake, new Rect(), other];
function total(): number {
  let t = 0;
  for (let i = 0; i < shapes.length; i++) {
    const s: Shape = shapes[i];
    t += s.describe();
  }
  return t;
}
console.log(total());
const only: Shape = fake;
console.log(only.describe(), only.area());
const solo: Shape = new Unrelated() as any as Shape;
console.log(solo.describe());
