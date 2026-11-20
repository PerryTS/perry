// Charter step 5, T1: a class whose constructor writes every `number` field
// from a parameter before anything can read it is born with F64 lanes for
// those fields, filled with +0.0 until the constructor stores. Nothing may
// observe that fill, and every writer that meets a non-Number or a
// non-finite value must take the checked path. Each section ends with a full
// collection: a runtime built with `field-rep-assert` checks every F64 lane
// at every trace.
declare function gc(): void;
function collect(): void {
  if (typeof gc === "function") gc();
}

class Vec2 {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
  setX(v: number): void {
    this.x = v;
  }
  len2(): number {
    return this.x * this.x + this.y * this.y;
  }
}

// 1. Numbers, then non-Numbers through the constructor prologue.
const odd: any[] = ["s", undefined, null, { n: 1 }, NaN, Infinity, -0, 7];
const made: Vec2[] = [];
for (let i = 0; i < 300; i++) made.push(new Vec2(i + 0.5, i * 2));
for (const v of odd) made.push(new Vec2(v as any, 1));
collect();
console.log("ctor", made.length, made[299].len2(), odd.map((v, i) => String(made[300 + i].x)).join(","));

// 2. The class-field store guard: primed with Numbers, then fed the rest.
const w = made.slice(0, 20);
for (let i = 0; i < 400; i++) w[i % 20].setX(i * 0.25);
for (let i = 0; i < odd.length; i++) w[i].setX(odd[i] as any);
collect();
console.log("guard", w.map((o) => String(o.x)).join(","));

// 3. A missing argument: the field is `undefined`, never the +0.0 fill.
const bare = new (Vec2 as any)();
collect();
console.log("bare", String(bare.x), String(bare.y), typeof bare.x);

// 4. A field read before its store: not F64 at birth; the read sees undefined.
class Peek {
  a: number;
  seen: string;
  constructor(a: number) {
    this.seen = String((this as any).a);
    this.a = a;
  }
}
const peeks: Peek[] = [];
for (let i = 0; i < 50; i++) peeks.push(new Peek(i));
collect();
console.log("peek", peeks[0].seen, peeks[49].a);

// 5. Subclass ordering: the base constructor calls an override that reads a
// subclass field before the subclass constructor stores it.
class Base {
  shown: string;
  constructor() {
    this.shown = this.show();
  }
  show(): string {
    return "base";
  }
}
class Derived extends Base {
  y: number;
  constructor(y: number) {
    super();
    this.y = y;
  }
  show(): string {
    return String(this.y);
  }
}
const ds: Derived[] = [];
for (let i = 0; i < 50; i++) ds.push(new Derived(i + 1));
collect();
console.log("derived", ds[0].shown, ds[49].y);

// 6. A subclass of a declared base: the base prologue runs through super().
class Point3 extends Vec2 {
  z: number;
  constructor(x: number, y: number, z: number) {
    super(x, y);
    this.z = z;
  }
}
const ps: Point3[] = [];
for (let i = 0; i < 100; i++) ps.push(new Point3(i, i + 1, i + 2));
ps[3].setX("str" as any);
ps[4].z = NaN;
collect();
console.log("sub", ps[99].len2(), ps[99].z, String(ps[3].x), String(ps[4].z));
