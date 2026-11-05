// #6968: a heap value stored into a SCALAR-REPLACED object field or array
// element must be a precise GC root.
//
// Escape analysis deletes the object and keeps one entry-block alloca per
// field/element. Those allocas belong to no HIR local, so the shadow-slot
// assignment pass (which walks `Stmt::Let`) never saw them and nothing bound
// them. With precise roots only, the value was swept out from under the
// alloca and the read returned recycled memory:
//
//   $ PERRY_CONSERVATIVE_STACK_SCAN=off PERRY_GC_HEAP_LIMIT=8 ./repro
//   obj f0-0 417894
//   obj  417894        <- o.a is gone
//
// The failure is a use-after-free, so the freed block has to be RECYCLED
// before it shows: one statement passes, six in a row do not. Each `fresh()`
// result is held only by the field alloca while `churn()` collects.
//
// Registered in test-parity/gc_repsel_corpus.txt; the `cons_scan_off` arm of
// scripts/gc_repsel_matrix.sh is the configuration that observes this.

let sink: unknown[] = [];

function churn(n: number): number {
  let acc = 0;
  for (let i = 0; i < n; i++) {
    sink.push({ i: i, s: "x" + (i & 255), a: [i, i + 1] });
    if (sink.length > 2048) {
      acc = (acc + sink.length) | 0;
      sink = [];
    }
  }
  return acc | 0;
}

function fresh(k: number): string {
  return "f" + k + "-" + (k * 7);
}

const N = 200000;

// --- scalar-replaced object literal: field `a` holds the only reference ----
{ const o = { a: fresh(0), b: churn(N) }; console.log("obj", o.a, o.b); }
{ const o = { a: fresh(1), b: churn(N) }; console.log("obj", o.a, o.b); }
{ const o = { a: fresh(2), b: churn(N) }; console.log("obj", o.a, o.b); }
{ const o = { a: fresh(3), b: churn(N) }; console.log("obj", o.a, o.b); }
{ const o = { a: fresh(4), b: churn(N) }; console.log("obj", o.a, o.b); }
{ const o = { a: fresh(5), b: churn(N) }; console.log("obj", o.a, o.b); }

// --- scalar-replaced array literal: element 0 holds the only reference -----
{ const a = [fresh(6), churn(N)]; console.log("arr", a[0], a[1]); }
{ const a = [fresh(7), churn(N)]; console.log("arr", a[0], a[1]); }
{ const a = [fresh(8), churn(N)]; console.log("arr", a[0], a[1]); }
{ const a = [fresh(9), churn(N)]; console.log("arr", a[0], a[1]); }
{ const a = [fresh(10), churn(N)]; console.log("arr", a[0], a[1]); }
{ const a = [fresh(11), churn(N)]; console.log("arr", a[0], a[1]); }

// --- a store AFTER construction writes the same alloca --------------------
{ const o = { a: "seed", b: 0 }; o.a = fresh(12); const n = churn(N); console.log("set", o.a, n); }
{ const o = { a: "seed", b: 0 }; o.a = fresh(13); const n = churn(N); console.log("set", o.a, n); }
{ const o = { a: "seed", b: 0 }; o.a = fresh(14); const n = churn(N); console.log("set", o.a, n); }
{ const o = { a: "seed", b: 0 }; o.a = fresh(15); const n = churn(N); console.log("set", o.a, n); }

// --- the numeric-only twin must keep working (it needs no rooting at all) --
{ const p = { x: 3, y: 4 }; console.log("num", (p.x * p.x + p.y * p.y) | 0); }
{ const q = [5, 12]; console.log("num", (q[0] * q[0] + q[1] * q[1]) | 0); }
