// Typed class accessor reads and writes on instances a constructor builds
// (they carry the class's family layout), through a typed binding, a
// function parameter and a closure, plus everything that must still miss:
// an own property shadowing the accessor, a subclass override, a replaced
// prototype accessor and a re-parented instance.

class Point {
  x: number;
  y: number;
  constructor(x: number, y: number) { this.x = x; this.y = y; }
  get sum() { return this.x + this.y; }
  get label() { return "P(" + this.x + "," + this.y + ")"; }
  set both(v: number) { this.x = v; this.y = v; }
}
class Point3 extends Point {
  z: number;
  constructor(x: number, y: number, z: number) { super(x, y); this.z = z; }
  get sum() { return super.sum + this.z; }
}

const p = new Point(1, 2);
const read = (q: Point) => q.sum;
const viaClosure = () => p.label;
let total = 0;
for (let i = 0; i < 1000; i++) total += read(p);
console.log("typed", p.sum, p.label, read(p), viaClosure(), total);
p.both = 5;
console.log("setter", p.x, p.y, p.sum);

const p3 = new Point3(1, 2, 3);
console.log("override", p3.sum, read(p3), p3.label);

const shadow = new Point(2, 3);
Object.defineProperty(shadow, "sum", { value: 100 });
console.log("own shadow", shadow.sum, read(shadow));

const late = new Point(4, 4);
late.x = 10;
(late as any).extra = 1;
console.log("late keys", late.sum, read(late));

Object.defineProperty(Point.prototype, "label", { get() { return "patched:" + (this as any).x; }, configurable: true });
console.log("patched proto", p.label, viaClosure(), p3.label);

const reparented = new Point(7, 8);
Object.setPrototypeOf(reparented, { get sum() { return -1; } });
console.log("reparented", (reparented as any).sum, read(reparented));
