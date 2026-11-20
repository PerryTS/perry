// CLS D1: a method replaced on the prototype after construction. The class-id
// switch of a typed or `this` receiver must not call the stale compiled body.
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

const shapes: Shape[] = [new Circle(), new Rect(), new Circle(), new Rect()];
function total(): number {
  let t = 0;
  for (let i = 0; i < shapes.length; i++) {
    const s: Shape = shapes[i];
    t += s.describe();
  }
  return t;
}
console.log(total());
(Circle.prototype as any).area = function (this: Shape) { return 10; };
console.log(total());
(Shape.prototype as any).area = function (this: Shape) { return 20; };
console.log(total());
