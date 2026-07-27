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
