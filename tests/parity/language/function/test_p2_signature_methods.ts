// P2 typed signatures (DESIGN-p2 rev 2.3 §5): the method controls. Exact
// against node with the fact on, off and in check mode.
// A loop bound the compiler cannot fold keeps each call site one HIR call
// (a static loop is unrolled, and its copies share the site's offset).
const RUN = Math.random() < 2 ? 1 : 0;

// S6: an override, so the site has two bodies (T0Set).
class A6 {
  v(x: number): number {
    return x + 1;
  }
}
class B6 extends A6 {
  v(x: number): number {
    return x + 2;
  }
}
const objs6: A6[] = [new A6(), new B6()];
let s6 = 0;
for (let i = 0; i < 10 * RUN; i++) s6 += objs6[i & 1].v(i);
console.log("S6", s6);

// S19: `.call` with another layout beside a typed `p.sum(i)` site; a
// subclass with another layout; an extracted method.
class P19 {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
  sum(k: number): number {
    return (this.x + this.y + k) | 0;
  }
}
const p19 = new P19(1, 2);
let s19 = 0;
for (let i = 0; i < 1000 * RUN; i++) s19 += p19.sum(i);
const q19 = { x: 10, z: 100, y: 20 };
console.log("S19", s19, P19.prototype.sum.call(q19, 3));
class Q19 extends P19 {
  w: number;
  constructor() {
    super(5, 6);
    this.w = 1;
  }
}
const ex19 = p19.sum;
console.log("S19", new Q19().sum(1), ex19.call(new P19(7, 8), 0));

// S24: a subclass overrides a method the typed body calls on `this`.
class R24 {
  base(): number {
    return 1;
  }
  calc(k: number): number {
    return this.base() + k;
  }
}
class S24 extends R24 {
  base(): number {
    return 100;
  }
}
const r24 = new R24();
let s24 = 0;
for (let i = 0; i < 10 * RUN; i++) s24 += r24.calc(i);
console.log("S24", s24, new S24().calc(1));

// T4c: layout breakers beside a typed `pp.sum(i)` site.
class P4c {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
  sum(k: number): number {
    return (this.x + this.y + k) | 0;
  }
}
const pp = new P4c(1, 2);
let t4c = 0;
for (let i = 0; i < 100 * RUN; i++) t4c += pp.sum(i);
const other = new P4c(3, 4);
delete (other as any).y;
console.log(
  "T4c",
  t4c,
  other.sum(1),
  Object.setPrototypeOf({ x: 10, z: 1, y: 20 }, P4c.prototype).sum(3),
  Object.create(P4c.prototype).sum(1),
);
