// CLS D1: Object.setPrototypeOf changes which method an instance resolves
// while its class id word is unchanged.
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

const shapes: Shape[] = [new Circle(), new Rect(), new Circle()];
function total(): number {
  let t = 0;
  for (let i = 0; i < shapes.length; i++) {
    const s: Shape = shapes[i];
    t += s.describe();
  }
  return t;
}
console.log(total());
Object.setPrototypeOf(shapes[0], Rect.prototype);
console.log(total());
Object.setPrototypeOf(shapes[2], { area() { return 50; }, describe() { return 500; } });
console.log(total());
