// A facts-closed method target is flattened into its caller. The receiver
// classes are proven, so the call is a direct call of the one body the
// classes resolve to; the semantics must be exactly the dispatch's.
class Shape {
  x = 0;
  name(): string {
    return "shape";
  }
  area(): number {
    return 0;
  }
  // Inherited by every subclass: the closed target of the loop below.
  describe(): string {
    return this.name() + ":" + this.area();
  }
  scaled(k: number): number {
    return this.area() * k;
  }
}

class Circle extends Shape {
  r: number;
  constructor(r: number) {
    super();
    this.r = r;
  }
  name(): string {
    return "circle";
  }
  area(): number {
    return 3 * this.r * this.r;
  }
}

class Rect extends Shape {
  w: number;
  h: number;
  constructor(w: number, h: number) {
    super();
    this.w = w;
    this.h = h;
  }
  name(): string {
    return "rect";
  }
  area(): number {
    return this.w * this.h;
  }
}

// Receivers the facts close over two classes: describe/scaled are direct,
// inlinable calls of Shape's bodies.
const shapes: Shape[] = [new Circle(2), new Rect(2, 3)];
let acc = "";
let total = 0;
for (let i = 0; i < 8; i++) {
  const s = shapes[i % 2];
  acc += s.describe() + ";";
  total += s.scaled(i);
}
console.log(acc);
console.log(total);

// Replacing an element keeps dispatching on the receiver.
shapes[0] = new Rect(5, 1);
let again = "";
for (let i = 0; i < 2; i++) {
  const s = shapes[i];
  again += s.describe() + ";";
}
console.log(again);

