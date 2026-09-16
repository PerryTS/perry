// Allocating an instance of an inheriting class re-registers its parent edge
// on every `new`. Skipping that when the edge is unchanged must not change what
// the chain answers: `instanceof` walks the registry, and so do method
// resolution, `super`, and the builtin-parent probes.

class Shape {
  kind: string;
  constructor(kind: string) {
    this.kind = kind;
  }
  describe(): string {
    return "shape:" + this.kind;
  }
}

class Rect extends Shape {
  w: number;
  h: number;
  constructor(w: number, h: number) {
    super("rect");
    this.w = w;
    this.h = h;
  }
  area(): number {
    return this.w * this.h;
  }
}

class Square extends Rect {
  constructor(s: number) {
    super(s, s);
  }
  describe(): string {
    return "square:" + super.describe();
  }
}

// Many allocations: the second and later ones take the skip.
const squares: Square[] = [];
for (let i = 0; i < 200; i++) {
  squares.push(new Square(i % 5));
}
const s = squares[3];
console.log("chain", s instanceof Square, s instanceof Rect, s instanceof Shape);
console.log("not", s instanceof Error, [] instanceof Shape);
console.log("methods", s.describe(), s.area(), s.kind, s.w, s.h);
console.log("count", squares.length, squares[199].area());

// A class expression built AFTER many allocations of the static chain: its
// edge is new, so it must publish normally.
const Dyn = class extends Rect {
  constructor() {
    super(2, 3);
  }
};
const d = new Dyn();
console.log("dyn", d instanceof Rect, d instanceof Shape, d.area(), d.kind);

// Two distinct children of one parent, interleaved with allocations.
class Circle extends Shape {
  r: number;
  constructor(r: number) {
    super("circle");
    this.r = r;
  }
}
for (let i = 0; i < 50; i++) {
  new Rect(i, i);
  new Circle(i);
}
const c = new Circle(7);
console.log("circle", c instanceof Circle, c instanceof Shape, c instanceof Rect, c.r, c.kind);

// Deep chain, allocated repeatedly.
class A1 {
  a = 1;
}
class B1 extends A1 {
  b = 2;
}
class C1 extends B1 {
  c = 3;
}
class D1 extends C1 {
  d = 4;
}
let deepSum = 0;
for (let i = 0; i < 100; i++) {
  const x = new D1();
  deepSum += x.a + x.b + x.c + x.d;
}
const deep = new D1();
console.log("deep", deepSum, deep instanceof A1, deep instanceof B1, deep instanceof C1, deep instanceof D1);

// Subclassing a builtin still resolves through the same registry.
class MyErr extends Error {
  code: number;
  constructor(code: number) {
    super("boom " + code);
    this.code = code;
  }
}
for (let i = 0; i < 20; i++) {
  new MyErr(i);
}
const e = new MyErr(9);
console.log("err", e instanceof MyErr, e instanceof Error, e.message, e.code);

// Prototype identity and getPrototypeOf agree with the chain.
console.log(
  "protos",
  Object.getPrototypeOf(Square.prototype) === Rect.prototype,
  Object.getPrototypeOf(Rect.prototype) === Shape.prototype,
  Object.getPrototypeOf(s) === Square.prototype,
);
