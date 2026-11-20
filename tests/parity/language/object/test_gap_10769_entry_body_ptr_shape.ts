// #10769: `Ptr<Shape>` in a PROGRAM-ENTRY / module-init body.
//
// The entry arm of `RepselContextFlags::derive` used to pin `allows_ptr_shape`
// off with a literal `false`, citing #6991 ("a compiled receiver goes stale
// across the globalThis-population collection, which runs around module init").
// #6991 was closed by #7249, which fixed it in the RUNTIME by putting
// `populate_global_this_builtins` inside a `GcSuppressScope`. The gate is now
// derived from its knob like any other body.
//
// `test_gap_repsel_ptr_shape_locals` CANNOT witness that change: its
// `Ptr<Shape>` selection count is identical with the gate on and off (18
// selected / 11 denied both ways), because every one of its candidates is
// either inside a function or module-globalized. This file exists because that
// one does not reach the lifted gate.
//
// NOTE ON WHAT IS NOT HERE. `Object.freeze` and `Object.defineProperty` are
// deliberately absent: either one arms the module-wide 5.2 shape-barrier kill
// (`ModuleDispatchFacts::has_shape_barrier_sites`), which disables ALL
// `Ptr<Shape>` promotion in the module. A first draft of this file included
// both and reported `0 selected / 15 denied` on BOTH arms -- it would have
// passed every GC run while witnessing nothing. Those cases belong in a module
// that is not trying to prove a shape; `test_gap_repsel_ptr_shape_barriers.ts`
// already owns them.
//
// Everything below is at TOP LEVEL on purpose — a binding read only from the
// entry body is not globalized, so it is a `Ptr<Shape>` candidate there and
// nowhere else. Each section puts a collection point between the proof and the
// use, which is the exact hazard #6991 named: the object may MOVE, so the
// tagged-at-rest slot must be re-derived after every safepoint.

const out: string[] = [];

// 1. Provenance-proven class instance in the entry body, with allocation
//    inside the loop so minors fire between the field reads.
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
    return this.x + this.y;
  }
}
const p = new Pt(3, 4, "origin");
let acc = 0;
const litter: number[][] = [];
for (let i = 0; i < 400; i++) {
  // allocate so the nursery fills and the back-edge poll collects
  litter.push([i, i + 1, i + 2]);
  if (litter.length > 32) litter.shift();
  p.x = i;
  acc = (acc + p.x + p.y + p.norm()) | 0;
}
out.push("1 class " + acc + " " + p.tag + " " + p.x + " " + p.y);

// 2. Anon-shape record literal in the entry body, read and written across a
//    call that allocates (a real safepoint between the proof and the use).
function churn(n: number): number {
  const tmp: string[] = [];
  for (let j = 0; j < n; j++) tmp.push("s" + j);
  return tmp.length;
}
const rec = { key: "k", value: 0, count: 0 };
let recAcc = 0;
for (let i = 0; i < 200; i++) {
  rec.value = i;
  const moved = churn(8); // allocates -> may collect -> `rec` may move
  rec.count = rec.count + moved;
  recAcc = (recAcc + rec.value + rec.count) | 0;
}
out.push("2 record " + recAcc + " " + rec.key + " " + rec.value + " " + rec.count);

// 3. Builder pattern in the entry body: `const b = {}` then fields added.
const builder: any = {};
builder.a = 1;
builder.b = 2;
let bAcc = 0;
for (let i = 0; i < 200; i++) {
  litter.push([i]);
  if (litter.length > 32) litter.shift();
  builder.a = i;
  bAcc = (bAcc + builder.a + builder.b) | 0;
}
out.push("3 builder " + bAcc + " " + builder.a + " " + builder.b);

// 4. A pointer-valued field written across a collection point — the write
//    barrier and the re-derived receiver have to agree.
const holder: any = { inner: null, n: 0 };
for (let i = 0; i < 200; i++) {
  holder.inner = { v: i, pad: "x".repeat(i % 7) };
  churn(4);
  holder.n = holder.n + holder.inner.v;
}
out.push("4 holder " + holder.n + " " + String(holder.inner.v));

// 5. The exclusions must stay byte-exact on the boxed/guarded protocol even
//    with the gate lifted: a reassigned local, a closure-captured local, and
//    an escaping local are all still ordinary.
let reassigned: any = { a: 1 };
reassigned = { a: 2, b: 3 };
out.push("5 reassigned " + reassigned.a + " " + String(reassigned.b));

const captured = { a: 10, b: 20 };
const readCaptured = (): number => captured.a + captured.b;
captured.a = 11;
out.push("5 captured " + readCaptured());

const escaping = { a: 100, b: 200 };
function consume(o: any): number {
  o.a = o.a + 1;
  return o.a + o.b;
}
out.push("5 escaping " + consume(escaping) + " " + escaping.a);

// 8. A deep chain read in the entry body across allocation.
const root = { mid: { leaf: { v: 7 } }, n: 0 };
for (let i = 0; i < 200; i++) {
  litter.push([i, i]);
  if (litter.length > 32) litter.shift();
  root.n = root.n + root.mid.leaf.v;
}
out.push("8 chain " + root.n + " " + root.mid.leaf.v);

console.log(out.join("\n"));
