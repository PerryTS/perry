// CLS D1: an own property shadows the prototype method on one instance. Both
// the main-loop call and `this.area()` inside describe must see it.
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
(shapes[0] as any).area = () => 100;
console.log(total());
(shapes[1] as any).describe = () => -1;
console.log(total());
