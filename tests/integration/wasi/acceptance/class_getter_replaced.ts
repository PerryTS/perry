// A class getter read compiles to a direct call of the declared accessor
// behind a shape guard, and a class method call to its declared body behind
// the holder's shadow bit. Replacing either on a prototype at run time, after
// those sites ran, must make every later read and call reach the new member.
class Point {
  x: number;
  y: number;
  constructor(x: number, y: number) { this.x = x; this.y = y; }
  get sum(): number { return this.x + this.y; }
  get label(): string { return "p" + this.x; }
  scale(): number { return this.x * 10; }
}
class Sub extends Point {}
function total(ps: Point[]): number { let t = 0; for (let i = 0; i < ps.length; i++) t += ps[i].sum; return t; }
function labels(ps: Point[]): string { let s = ""; for (let i = 0; i < ps.length; i++) s += ps[i].label; return s; }
function scales(ps: Point[]): number { let t = 0; for (let i = 0; i < ps.length; i++) t += ps[i].scale(); return t; }
function readScale(p: Point): string { return typeof p.scale; }
const ps = [new Point(1, 2), new Point(3, 4)];
const ss = [new Sub(5, 6)];
for (let r = 0; r < 3; r++) console.log(total(ps), total(ss), labels(ps), scales(ps), scales(ss));
Object.defineProperty(Point.prototype, "sum", { get() { return this.x * this.y; }, configurable: true });
console.log(total(ps), total(ss));
Object.defineProperty(Sub.prototype, "sum", { get() { return -1; }, configurable: true });
console.log(total(ps), total(ss));
const getLabel = Object.getOwnPropertyDescriptor(Point.prototype, "label")!.get!;
Object.defineProperty(Point.prototype, "label", { get() { return "q" + getLabel.call(this); }, configurable: true });
console.log(labels(ps), labels(ss));
(Point.prototype as any).scale = function (this: Point) { return this.y; };
console.log(scales(ps), scales(ss), readScale(ps[0]));
(Sub.prototype as any).scale = function () { return 100; };
console.log(scales(ps), scales(ss));
