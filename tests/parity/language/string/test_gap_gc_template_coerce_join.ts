// #11604: S2 (#11554) split `${v}` into an inline string/SSO tag test plus the
// collecting `js_template_string_coerce_box` on a cold arm, joined by
//
//   %p = phi double [ %v, %entry ], [ %coerced, %tmpl_coerce.slow ]
//
// When `v` is itself a heap value the gate anchors on -- `${x.toString()}`
// lowers `v` to `js_jsvalue_to_string_method_box` -- the root-dominance check
// used to follow the phi back to `v` and count the slow arm's call as a
// collection between `v` and its root store: 96 violations on the
// dependency-scale corpus, none in the curated one. On the path through the
// slow arm the slot receives `%coerced`, not `v`, so that path was never a
// window for `v`. This file puts the shape in the curated corpus
// (`test_gap_gc_*` is one of gc_root_dominance_corpus.sh's patterns), so the
// checker's phi-edge refinement is exercised on real emitted IR and not only
// on its self-test fixtures.
//
// It also runs the shape under the moving collector. Every template below
// sits in a loop that allocates, so a back-edge poll has something to move:
//
//   PERRY_GC_SCHEDULE_SEED=7 PERRY_GC_SCHEDULE_RATE=1 \
//   PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_DIAG=1 ./out
//
// must print the same lines as node, with `copying_minors` above zero.

class Point {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}

// Operands of every class: a `.toString()` result (heap value into the join's
// fast edge), a number (slow arm allocates its text), an object with a user
// `toString` (slow arm runs JS), and an already-string local.
function label(n: number, o: any, s: string): string {
  const a = `${n.toString()}:${o.toString()}`;
  const b = `${n}|${o}|${a}|${s}`;
  return a + "/" + b;
}

const kept: string[] = [];
let total = 0;
for (let i = 0; i < 6000; i++) {
  const p = new Point(i, i * 2);
  const o = {
    k: i,
    toString() {
      return "o" + this.k;
    },
  };
  const r = label(i + 0.5, o, "s" + i) + `${p.x.toString()}-${[i, i + 1].toString()}`;
  total += r.length;
  if (i % 1500 === 0) kept.push(r);
}

// Values bound across the loop must still read correctly after every move.
for (const k of kept) console.log(k);
console.log("total", total);
