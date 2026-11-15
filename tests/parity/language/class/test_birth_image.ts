// Birth cost: a complete constructor-free construction (a closed object
// literal, or a class whose constructor is exactly `this.f = p` for every
// field) is born as its image: the nursery arm writes the header and every
// slot's final word, with no shared fill. Every case below either is such a
// birth with something that could go wrong in the window (an allocating,
// collecting or throwing initializer, a moved argument, a value that is no
// plain number, int lanes, a nursery overflow in a loop), or is a negative
// control that must keep the ordinary birth (a constructor that throws
// midway or leaks `this`, a subclass, accessors, a field added later,
// computed and spread keys). A slot read before its write, or a stale
// argument, shows up as output.

// `keep` makes the objects escape, so each `new` is a real birth.
const keep: unknown[] = [];
let calls: string[] = [];
function note<T>(tag: string, v: T): T {
  calls.push(tag);
  return v;
}
function churn(n: number): number[] {
  let last: number[] = [];
  for (let i = 0; i < n; i++) last = [i, i + 1];
  return last;
}

// 1. The second initializer allocates and collects.
for (let i = 0; i < 300; i++) {
  const r = { a: i, b: churn(50), c: i + 1, d: i * 2, e: "e" + i, f: i % 3 === 0 };
  if (r.a !== i || r.b[1] !== 50 || r.c !== i + 1 || r.d !== 2 * i || r.e !== "e" + i) {
    throw new Error("alloc " + i);
  }
  keep.push(r);
}
console.log("alloc ok");

// 2. A throwing initializer: no partial object is visible.
function boom(): number {
  throw new Error("boom");
}
let seen: unknown = "unset";
try {
  seen = { a: note("a", 1), b: boom(), c: note("c", 3), d: 4, e: 5 };
} catch (e) {
  console.log("caught", (e as Error).message, seen, calls.join(""));
}

// 3. A getter read in an initializer; a getter on the literal itself.
const src = {
  get g(): number {
    calls.push("g");
    return 7;
  },
};
calls = [];
const viaGetter = { a: src.g, b: 2, c: 3, d: 4, e: src.g + 1 };
console.log(viaGetter.a, viaGetter.e, calls.join(""));
const withGetter = {
  get x(): number {
    return this.y * 2;
  },
  y: 21,
};
console.log(withGetter.x);

// 4. `this` captured by a literal's arrow and method.
class Holder {
  tag = "holder";
  make() {
    return {
      a: 1,
      b: 2,
      c: 3,
      d: 4,
      who: () => this.tag,
      self() {
        return this.a + this.d;
      },
    };
  }
}
const made = new Holder().make();
console.log(made.who(), made.self());

// 5. Negative: a constructor that leaks `this` between its stores.
const registry: any[] = [];
class Registers {
  a: number;
  b: number;
  c: number;
  d: number;
  e: number;
  constructor(a: number, b: number, c: number, d: number, e: number) {
    this.a = a;
    this.b = b;
    registry.push(this);
    registry.push(Object.keys(this).join(""));
    this.c = c;
    this.d = d;
    this.e = e;
  }
}
const reg = new Registers(1, 2, 3, 4, 5);
console.log(registry[0] === reg, registry[1], reg.e);

// 6. Negative: a constructor that throws midway; the partial object leaked.
let partial: any = null;
class Midway {
  a: number;
  b: number;
  c: number;
  d: number;
  e: number;
  constructor(a: number, b: number, c: number, d: number, e: number) {
    this.a = a;
    this.b = b;
    partial = this;
    if (c < 0) throw new Error("midway");
    this.c = c;
    this.d = d;
    this.e = e;
  }
}
try {
  new Midway(1, 2, -3, 4, 5);
} catch (e) {
  console.log((e as Error).message, partial.a, partial.b, partial.c, Object.keys(partial).join(""));
}
console.log(new Midway(1, 2, 3, 4, 5).e);

// 7. Negative: computed and spread keys.
const base = { p: 1, q: 2 };
const k = "dyn";
const spread = { ...base, r: 3, s: 4, t: 5 };
const computed = { [k]: 1, a: 2, b: 3, c: 4, d: 5 };
console.log(JSON.stringify(spread), JSON.stringify(computed));

// 8. Nested literals.
let nestedSum = 0;
for (let i = 0; i < 200; i++) {
  const n = { inner: { x: i, y: i + 0.5, z: [i] }, a: i, b: i, c: i, d: { w: i } };
  nestedSum += n.inner.x + n.inner.y + n.inner.z[0] + n.d.w + n.c;
  keep.push(n);
}
console.log(nestedSum);

// 9. Values that are no plain number into fields born numeric.
class Wide {
  x: number;
  a: number;
  b: number;
  c: number;
  d: number;
  constructor(x: number, a: number, b: number, c: number, d: number) {
    this.x = x;
    this.a = a;
    this.b = b;
    this.c = c;
    this.d = d;
  }
}
const odd: any[] = [1, "two", NaN, undefined, { v: 1 }, Infinity, -0, 2.5, null, -Infinity];
console.log(odd.map((v) => String(new Wide(v, 1, 2, 3, 4).x)).join(","));
let wideSum = 0;
for (let i = 0; i < 1000; i++) {
  const w = new Wide(i, i, i, i, i + 0.25);
  keep.push(w);
  wideSum += w.d;
}
const odds = odd.map((v) => new Wide(v, 1, 2, 3, 4));
keep.push(odds);
console.log(odds.map((w) => String(w.x) + Object.is(w.x, -0)).join(","));
console.log(wideSum);

// 10. Mixed kinds: strings, arrays, booleans, a self-typed pointer.
class Mixed {
  name: string;
  list: number[];
  n: number;
  flag: boolean;
  next: Mixed | null;
  constructor(name: string, list: number[], n: number, flag: boolean, next: Mixed | null) {
    this.name = name;
    this.list = list;
    this.n = n;
    this.flag = flag;
    this.next = next;
  }
}
let chain: Mixed | null = null;
for (let i = 0; i < 100; i++) {
  chain = new Mixed("m" + i, [i, i], i, i % 2 === 0, chain);
  keep.push(chain);
}
let len = 0;
for (let c = chain; c; c = c.next) len += c.list.length + (c.flag ? 1 : 0);
console.log(chain!.name, len);

// 11. Negative: a setter of the class intercepts a constructor store.
const logged: unknown[] = [];
class WithSetter {
  b: number;
  c: number;
  d: number;
  e: number;
  set pa(v: number) {
    logged.push(v);
  }
  get twice(): number {
    return this.b * 2;
  }
  constructor(a: number, b: number, c: number, d: number, e: number) {
    this.pa = a;
    this.b = b;
    this.c = c;
    this.d = d;
    this.e = e;
  }
}
const ws = new WithSetter(1, 2, 3, 4, 5);
console.log(logged.join(","), Object.keys(ws).join(","), ws.e, ws.twice);

// 12. Negative: a subclass (its parent's constructor runs between).
class Base5 {
  a: number;
  b: number;
  c: number;
  constructor(a: number, b: number, c: number) {
    this.a = a;
    this.b = b;
    this.c = c;
  }
}
class Derived5 extends Base5 {
  d: number;
  e: number;
  f: number;
  constructor(a: number, b: number, c: number, d: number, e: number, f: number) {
    super(a, b, c);
    this.d = d;
    this.e = e;
    this.f = f;
  }
}
let dsum = 0;
for (let i = 0; i < 300; i++) {
  const d = new Derived5(i, i + 1, i + 2, i + 3, i + 4, i + 5);
  keep.push(d);
  dsum += d.a + d.c + d.f;
}
console.log(dsum, Object.keys(new Derived5(1, 2, 3, 4, 5, 6)).join(""));

// 13. A field added later: the image class stays as it was born; the
// literal that gets a late key is no fixed layout.
const late: any = { a: 1, b: 2, c: 3, d: 4, e: 5 };
late.z = 26;
const fixed = { a: 1, b: 2, c: 3, d: 4, e: 5, f: 6 };
console.log(Object.keys(late).join(""), late.z, Object.keys(fixed).join(""), fixed.f);
class Grows {
  a: number;
  b: number;
  c: number;
  d: number;
  e: number;
  constructor(a: number, b: number, c: number, d: number, e: number) {
    this.a = a;
    this.b = b;
    this.c = c;
    this.d = d;
    this.e = e;
  }
}
const g: any = new Grows(1, 2, 3, 4, 5);
g.extra = "x";
delete g.b;
console.log(Object.keys(g).join(","), g.extra, g.b);

// 14. Int lanes: an object literal and a class whose int fields are I32
// at rest, copied from one birth into the next (the chess `Undo` shape),
// across thousands of nursery overflows.
interface Snap {
  move: number | undefined;
  piece: number;
  captured: number;
  square: number;
  rights: number;
  ep: number;
  half: number;
  full: number;
  wk: number;
  bk: number;
  ha: number;
  hb: number;
  ctx: number;
  tracked: boolean;
}
class Pos {
  rights = 15;
  ep = -1;
  half = 0;
  full = 1;
  wk = 4;
  bk = 116;
  ha = 0x1234567;
  hb = -0x7654321;
  ctx = 0;
  stack: Snap[] = [];
  make(move: number, tracked: boolean): Snap {
    const piece = (move & 7) - 3;
    const snap: Snap = {
      move, piece, captured: (move >> 3) & 7, square: move & 127, rights: this.rights,
      ep: this.ep, half: this.half, full: this.full,
      wk: this.wk, bk: this.bk, ha: this.ha, hb: this.hb,
      ctx: this.ctx, tracked,
    };
    this.rights = (this.rights ^ move) & 15;
    this.ep = (move & 8) !== 0 ? move & 127 : -1;
    this.half = piece === 1 ? 0 : this.half + 1;
    this.full += 0.5;
    this.wk = (this.wk + move) & 127;
    this.bk = (this.bk ^ move) & 127;
    this.ha = (this.ha ^ (move * 2654435761)) | 0;
    this.hb = (this.hb + move * 40503) | 0;
    this.ctx = (this.ctx + 1) | 0;
    return snap;
  }
  unmake(u: Snap): void {
    this.rights = u.rights;
    this.ep = u.ep;
    this.half = u.half;
    this.full = u.full;
    this.wk = u.wk;
    this.bk = u.bk;
    this.ha = u.ha;
    this.hb = u.hb;
    this.ctx = u.ctx;
  }
}
const pos = new Pos();
let acc = 0;
const depth: Snap[] = [];
for (let i = 0; i < 120000; i++) {
  const u = pos.make((i * 7919) & 0xffff, (i & 1) === 0);
  acc = (acc + u.piece + u.captured + u.rights + u.wk + u.bk + (u.ha ^ u.hb) + u.ctx) | 0;
  if (u.move !== ((i * 7919) & 0xffff) || u.tracked !== ((i & 1) === 0)) throw new Error("snap " + i);
  depth.push(u);
  if (depth.length === 64) {
    while (depth.length > 3) pos.unmake(depth.pop()!);
  }
  if (i % 30000 === 0) pos.stack.push(u);
}
console.log(acc, pos.rights, pos.ep, pos.half, pos.full, pos.wk, pos.bk, pos.ha, pos.hb, pos.ctx);
console.log(pos.stack.map((u) => [u.move, u.piece, u.ep, u.full, u.ha, u.tracked].join("/")).join(" "));

// 15. An argument object lowered before an argument whose evaluation
// collects: the image must store the moved object.
class Moved {
  n: number;
  o: { k: number };
  m: number;
  p: number;
  q: number;
  constructor(n: number, o: { k: number }, m: number, p: number, q: number) {
    this.n = n;
    this.o = o;
    this.m = m;
    this.p = p;
    this.q = q;
  }
}
function churnObjects(): number {
  let x: any;
  for (let i = 0; i < 20000; i++) x = { i };
  return x.i;
}
let total = 0;
for (let i = 0; i < 400; i++) {
  const u = new Moved(i, { k: i }, churnObjects(), 1.5, 2.5);
  if (u.o.k !== i) throw new Error("stale " + i);
  total += u.o.k + u.m + u.p + u.q;
  const r = { a: i, o: { k: i }, m: churnObjects(), p: 1.5, q: 2.5, s: "s" + i };
  if (r.o.k !== i || r.s !== "s" + i) throw new Error("stale literal " + i);
  if (i % 100 === 0) keep.push(r, u);
}
console.log(total, keep.length);
