// Representation-selection Phase 3b: shape-proven object locals
// (PERRY_PTR_SHAPE_LOCALS, RFC docs/representation-selection-rfc.md §5.5-§5.7).
//
// Exercises the Ptr<Shape> promotion seams against Node:
//  - a provenance-proven `new C(...)` local: guard-free field reads in a hot
//    loop, field writes (scalar and pointer-valued), direct method calls,
//  - safepoints inside the loop (allocation + call) so the tagged-at-rest
//    slot must be re-derived after each safepoint (GC may move the object),
//  - an anon-shape record literal ({key, value} — the #6904 reporting shape),
//  - the builder pattern (`const b = {}; b.a = …`),
//  - exclusions that must stay byte-exact on the boxed/guarded protocol:
//    reassigned locals, closure-referenced locals, escaping locals.

// 1. Provenance-proven class instance: field sum loop + writes + method calls.
class Pt {
  x: number;
  y: number;
  tag: string;
  constructor(x: number, y: number, tag: string) {
    this.x = x;
    this.y = y;
    this.tag = tag;
  }
  norm(): number {
    return this.x * this.x + this.y * this.y;
  }
  label(): string {
    return this.tag + ":" + (this.x + this.y);
  }
}

function fieldSum(n: number): string {
  const o = new Pt(1.5, 2.25, "p");
  let acc = 0;
  for (let i = 0; i < n; i++) {
    acc += o.x + o.y;
  }
  o.x = acc / n;
  o.y = o.norm();
  o.tag = o.label();
  return o.tag + "|" + o.x + "|" + o.y;
}
console.log(fieldSum(1000));

// 2. Safepoints in the loop body: allocation pressure forces minor GCs while
//    the proven local is live — the object moves, the slot is rewritten, and
//    every post-safepoint access must re-derive the pointer.
class Acc {
  total: number;
  count: number;
  constructor() {
    this.total = 0;
    this.count = 0;
  }
  add(v: number): void {
    this.total += v;
    this.count++;
  }
}

function churn(n: number): string {
  const a = new Acc();
  for (let i = 0; i < n; i++) {
    const garbage = new Array(64).fill(i); // allocation safepoint
    a.add(garbage[i % 64]);
    if (i % 97 === 0) {
      a.total = a.total + garbage.length;
    }
  }
  return a.total + "/" + a.count;
}
console.log(churn(5000));

// 3. Anon-shape record literals (issue #6904's reporting shape).
function records(): string {
  const src: [string, number][] = [
    ["alpha", 3],
    ["beta", 1],
    ["gamma", 4],
    ["delta", 1],
  ];
  let out = "";
  for (const [k, v] of src) {
    const rec = { key: k, value: v };
    rec.value = rec.value * 2 + rec.key.length;
    out += rec.key + "=" + rec.value + ";";
  }
  return out;
}
console.log(records());

// 4. Builder pattern (`{}`-site class; object-write matrix w15 residual).
function builder(): string {
  const b: { [k: string]: number } = {};
  b.first = 1;
  b.second = 2;
  b.third = 3;
  let s = 0;
  for (let i = 0; i < 100; i++) {
    s += b.first + b.second + b.third;
  }
  return s + ":" + JSON.stringify(b);
}
console.log(builder());

// 5. Reassigned local: NOT provenance-stable, must stay on the guarded path.
function reassigned(flag: boolean): number {
  let o = new Pt(1, 2, "a");
  if (flag) {
    o = new Pt(10, 20, "b");
  }
  let acc = 0;
  for (let i = 0; i < 10; i++) acc += o.x + o.y;
  return acc;
}
console.log(reassigned(true), reassigned(false));

// 6. Closure-referenced local: excluded (capture machinery stays boxed).
function capturedObj(): number {
  const o = new Pt(3, 4, "c");
  const f = () => o.norm();
  let acc = 0;
  for (let i = 0; i < 5; i++) acc += o.x;
  return acc + f();
}
console.log(capturedObj());

// 7. Escaping local (returned): escape_news must reject it.
function escapes(): Pt {
  const o = new Pt(5, 6, "d");
  o.x += 1;
  return o;
}
const escaped = escapes();
console.log(escaped.x, escaped.y, escaped.label());

// 8. Two proven locals of the same class alive at once (distinct identities).
function twoLocals(): string {
  const p = new Pt(1, 1, "p");
  const q = new Pt(2, 2, "q");
  let acc = 0;
  for (let i = 0; i < 50; i++) {
    acc += p.x + q.y;
  }
  p.x = q.x;
  return acc + ":" + p.x + ":" + q.label();
}
console.log(twoLocals());

// 9. Proven local passed BY FIELD VALUE to boxed consumers (materialization
//    at the boundary): console.log observes exact values.
function boundary(): void {
  const o = new Pt(0.1, 0.2, "b");
  console.log(o.x + o.y); // classic 0.30000000000000004
  console.log(o.norm());
  const arr = [o.x, o.y];
  console.log(arr.join(","));
}
boundary();

// 10. Extends chain (target-1 widening): parent field offsets are chain-
//     global; the straight-line method earns a typed-receiver clone.
class BasePos {
  offset: number;
  constructor(offset: number) {
    this.offset = offset;
  }
}
class Scaler extends BasePos {
  factor: number;
  constructor(offset: number, factor: number) {
    super(offset);
    this.factor = factor;
  }
  apply(x: number): number {
    return x * this.factor + this.offset;
  }
}
function chainRun(n: number): string {
  const s = new Scaler(3, 1.5);
  let acc = 0;
  for (let i = 0; i < n; i++) {
    acc += s.apply(i);
  }
  s.offset = acc / n;
  return acc + ":" + s.offset + ":" + s.factor + ":" + s.apply(2);
}
console.log(chainRun(1000));

// 11. Internal `this.m(...)` argument sites: `set` is called externally with
//     a number AND internally (via poke) with a string smuggled through an
//     `any` param. The callee's params must stay numeric-unproven, so reads
//     after the internal call observe the string exactly.
class Cnt {
  total: number;
  constructor() {
    this.total = 0;
  }
  set(v: number): void {
    this.total = v;
  }
  poke(x: any): void {
    this.set(x);
  }
}
function internalPoison(): string {
  const c = new Cnt();
  c.set(5);
  let acc = 0;
  for (let i = 0; i < 8; i++) acc += c.total;
  c.poke("12" as any); // numeric STRING: ToNumber("12")=12, raw bits != 12
  const t = Math.trunc(c.total as any); // spec ToNumber("12") -> 12
  const after = c.total;
  return acc + ":" + t + ":" + after + ":" + typeof after;
}
console.log(internalPoison());

// ─────────────────────────────────────────────────────────────────────────
// Representation-selection Phase 5a: proven `this` in methods
// (PERRY_PTR_SHAPE_THIS). Both call sites that already prove a receiver's
// exact shape route to the internal `__pshape` clone, whose `this.field`
// accesses lower guard-free. The frozen-receiver case lives in its own file
// (test_gap_repsel_proven_this_frozen.ts) because the freeze-family kill is
// module-wide by construction — a single Object.freeze here would disable
// every write-containing clone in THIS file and hide the cases below.
// ─────────────────────────────────────────────────────────────────────────

// 12. The 09_method_calls shape: a module-scope receiver (NOT a Phase 3b
//     local — module globals are excluded), so the call goes through the
//     guarded `method_direct.fast` arm. That guard proves class id + keys
//     token, and Phase 5a routes it to the proven-`this` clone.
class Counter5a {
  value: number;
  constructor() {
    this.value = 0;
  }
  increment(): void {
    this.value = this.value + 1;
  }
  bump(by: number): void {
    this.value = this.value + by;
  }
  get(): number {
    return this.value;
  }
}
const counter5a = new Counter5a();
for (let i = 0; i < 2000; i++) {
  counter5a.increment();
}
counter5a.bump(1.5);
console.log("p5a-counter:" + counter5a.get() + ":" + typeof counter5a.get());

// 13. Proven `this` read + write + a `this.m()` method chain, reached from a
//     Phase 3b local (the guard-free routing site).
class Vec5a {
  x: number;
  y: number;
  scale: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
    this.scale = 1;
  }
  lenSq(): number {
    return this.x * this.x + this.y * this.y;
  }
  // `this.lenSq()` is an internal method chain: vetted transitively.
  normalizeTo(target: number): number {
    const l = this.lenSq();
    this.scale = target / (l === 0 ? 1 : l);
    this.x = this.x * this.scale;
    this.y = this.y * this.scale;
    return this.scale;
  }
  describe(): string {
    return this.x + "," + this.y + "," + this.scale;
  }
}
function provenThisChain(n: number): string {
  const v = new Vec5a(3, 4);
  let acc = 0;
  for (let i = 0; i < n; i++) {
    acc += v.lenSq();
  }
  const s = v.normalizeTo(2);
  return acc + "|" + s + "|" + v.describe();
}
console.log("p5a-chain:" + provenThisChain(500));

// 14. `this` ESCAPE rejection: a method that hands `this` out as a value can
//     alias the receiver, so no clone may be emitted and every access must
//     keep the guarded lowering. Output must be byte-exact either way.
const escaped5a: any[] = [];
class Leaky5a {
  n: number;
  constructor(n: number) {
    this.n = n;
  }
  leak(): void {
    escaped5a.push(this); // `this` in value position — disqualifies
    this.n = this.n + 1;
  }
  read(): number {
    return this.n;
  }
}
function leakRun(): string {
  const l = new Leaky5a(7);
  l.leak();
  l.leak();
  // The alias observes the same object identity and the same mutations.
  const same = escaped5a[0] === escaped5a[1] && escaped5a[0] === l;
  (escaped5a[0] as Leaky5a).n = 99;
  return l.read() + ":" + same + ":" + escaped5a.length;
}
console.log("p5a-leak:" + leakRun());

// 15. Closure-capturing-`this` rejection: an arrow function in the body
//     captures `this`, which creates an alias the walk cannot follow.
class Closed5a {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
  addAll(xs: number[]): number {
    xs.forEach((x) => {
      this.v = this.v + x; // captures_this — disqualifies the clone
    });
    return this.v;
  }
}
function closureThis(): string {
  const c = new Closed5a(10);
  const r = c.addAll([1, 2, 3, 4]);
  return r + ":" + c.v;
}
console.log("p5a-closure:" + closureThis());

// 16. Static-method exclusion: statics have no receiver at all, and the
//     `perry_static_` targets must never be routed to a `__pshape` symbol.
class Stat5a {
  static base: number = 5;
  static twice(x: number): number {
    return x * 2 + Stat5a.base;
  }
  inst: number;
  constructor() {
    this.inst = Stat5a.twice(3);
  }
  read(): number {
    return this.inst + Stat5a.twice(1);
  }
}
function staticRun(): string {
  const s = new Stat5a();
  return Stat5a.twice(10) + ":" + s.read() + ":" + s.inst;
}
console.log("p5a-static:" + staticRun());

// 17. Interface-annotated local with `new` provenance (the predicates.rs
//     narrowing): `const o: Shaped5a = new Impl5a()` must KEEP its Ptr<Shape>
//     proof — `Shaped5a` names no class, so the declared type carries
//     strictly less information than the provenance.
interface Shaped5a {
  w: number;
  area(): number;
}
class Impl5a implements Shaped5a {
  w: number;
  h: number;
  constructor(w: number, h: number) {
    this.w = w;
    this.h = h;
  }
  area(): number {
    return this.w * this.h;
  }
}
function ifaceLocal(n: number): string {
  const o: Shaped5a = new Impl5a(3, 4);
  let acc = 0;
  for (let i = 0; i < n; i++) {
    acc += o.area() + o.w;
  }
  return acc + ":" + o.w + ":" + o.area();
}
console.log("p5a-iface:" + ifaceLocal(250));

// 18. An explicit CLASS annotation still wins over provenance (the half of
//     the old rule the narrowing preserves).
class Named5a {
  k: number;
  constructor(k: number) {
    this.k = k;
  }
  twice(): number {
    return this.k * 2;
  }
}
function namedLocal(): string {
  const o: Named5a = new Named5a(21);
  return o.twice() + ":" + o.k;
}
console.log("p5a-named:" + namedLocal());

// 19. Subclass receiver on an INHERITED method: the clone is compiled for
//     the declaring class, so an inherited call must NOT route to it. The
//     subclass adds a field, so chain-global indexes differ from the base's.
class Base5a {
  a: number;
  constructor(a: number) {
    this.a = a;
  }
  readA(): number {
    return this.a * 10;
  }
}
class Derived5a extends Base5a {
  b: number;
  constructor(a: number, b: number) {
    super(a);
    this.b = b;
  }
  sum(): number {
    return this.a + this.b;
  }
}
function inherited(): string {
  const base = new Base5a(1);
  const d = new Derived5a(2, 3);
  // `d.readA()` resolves to Base5a::readA through the chain walk.
  return base.readA() + ":" + d.readA() + ":" + d.sum() + ":" + d.a + ":" + d.b;
}
console.log("p5a-inherit:" + inherited());
