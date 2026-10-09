// Refs #12240: a typed direct method call warmed against a class's compiled
// body must observe every later change to a prototype holder on its path:
// a method added or replaced on the declaring class, a nearer shadow on an
// intermediate class, a descriptor install, Object.assign / Reflect.set
// onto the prototype, and a write that reaches the prototype through
// `this.constructor.prototype`. Run both default and PERRY_TYPED_FEEDBACK=1
// builds: the latter puts js_typed_feedback_method_direct_call_guard first.
class Shape {
  w = 2;
  area(): number { return this.w * 10; }
  label(): string { return "shape"; }
}
class Square extends Shape { s = 1; }
class Tile extends Square { t = 3; }
class Lazy { v = 5; get(): number { return this.v; } }

function area(o: Shape): number { return o.area(); }
function squareArea(o: Square): number { return o.area(); }
function tileLabel(o: Tile): string { return o.label(); }
function lazyGet(o: Lazy): number { return o.get(); }

const shape = new Shape();
const square = new Square();
const tile = new Tile();
const lazy = new Lazy();

function round(tag: string) {
  let a = 0, b = 0, c = "", d = 0;
  for (let i = 0; i < 200; i++) {
    a += area(shape);
    b += squareArea(square);
    c = tileLabel(tile);
    d += lazyGet(lazy);
  }
  console.log(tag, a, b, c, d);
}

round("warm");
round("warm2");

// Replace on the declaring class.
const originalArea = Shape.prototype.area;
Shape.prototype.area = function (this: Shape) { return this.w * 100; };
round("replace-declaring");

// A nearer shadow on an intermediate class; Shape stays as it was.
(Square.prototype as any).area = function () { return 7; };
round("shadow-intermediate");
delete (Square.prototype as any).area;
round("shadow-deleted");

// Restore and re-warm, then a descriptor install.
Shape.prototype.area = originalArea;
round("restored");
Object.defineProperty(Shape.prototype, "area", {
  configurable: true,
  writable: true,
  value: function () { return -1; },
});
round("define-value");
Object.defineProperty(Shape.prototype, "area", {
  configurable: true,
  get() { return function () { return -2; }; },
});
round("define-getter");
Object.defineProperty(Shape.prototype, "area", { configurable: true, writable: true, value: originalArea });
round("define-restored");

// Object.assign and Reflect.set onto prototypes along the path.
Object.assign(Tile.prototype, { label() { return "assigned"; } });
round("assign-leaf");
Reflect.set(Square.prototype, "label", function () { return "reflected"; });
delete (Tile.prototype as any).label;
round("reflect-intermediate");
delete (Square.prototype as any).label;
round("label-restored");

// A prototype never read before the warm-up (lazy holder), patched through
// the instance's constructor.
(lazy.constructor as any).prototype.get = function () { return 42; };
round("lazy-holder-patched");

// A prototype method that did not exist at compile time, added after warm-up
// and then shadowing nothing until a subclass instance reads it.
(Shape.prototype as any).extra = function () { return "extra"; };
console.log("extra", (tile as any).extra(), Object.keys(Shape.prototype).join(","));
