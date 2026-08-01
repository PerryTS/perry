// Liveness fixture for the `Ptr<Shape>` ARRAY-ELEMENT escape (#7034 §3).
//
// The other two `ptr_shape` fixtures prove that a *contained* local promotes
// and that each consumption site fires. Neither of them touches an array, so
// both stay green if `collectors/ptr_shape_elements.rs` stops issuing facts
// entirely — and the 18 real corpus workloads promote zero element locals
// today, so the corpus cannot see it either. Without this file the element
// rule has no gate at all: it would be exactly CLAUDE.md failure mode 4, a
// green job whose subject never ran.
//
// What this program has to get right, all at once:
//
//   1. `rows` must satisfy every element-array conjunct: one `const rows = []`
//      binding, only `push` writes of one class, only `.length` and in-bounds
//      `rows[i]` reads, and no other use at all. It is deliberately NOT
//      returned — `return rows` is admitted by the rule, but returning it
//      would let the deforestation pass (`perry-transform/src/deforest`)
//      rewrite the local array into a `__deforest_out` PARAMETER, which this
//      analysis cannot see. That is a real coverage hole (it is why
//      `batch.ts` is unchanged by #7034 §3) and it must not silently make
//      this fixture vacuous.
//   2. The producer local `row` must escape ONLY through the push, so its
//      promotion is attributable to the element exemption and to nothing
//      else. Its field store before the push is what keeps it out of scalar
//      replacement (#7115) — without it the object is deleted outright and
//      no access site is reached.
//   3. Both read forms must appear: the explicit `const s = rows[i]` inside a
//      `i < rows.length` loop, and the `for (const r of rows)` iterator form,
//      which desugars to the same shape. If the desugar ever changes, this
//      fixture's count drops and the gate goes red — which is the point.
//   4. Every read must be a declared field of `Row`, and no member of the
//      group may escape: one `r.extra = 1` anywhere voids the WHOLE group by
//      design, and would take this fixture to zero.
//
// Do not "tidy" this file. In particular do not add `return rows`, do not
// hoist the `new Row(...)` into the `push` call (that removes the producer
// local this fixture is here to promote), and do not merge the two read
// loops.

class Row {
  id: number;
  weight: number;
  score: number;
  constructor(id: number, weight: number) {
    this.id = id;
    this.weight = weight;
    this.score = 0;
  }
  rescore(f: number): number {
    return this.weight * f + this.id;
  }
}

function build(n: number): number {
  const rows: Row[] = [];
  for (let i = 0; i < n; i++) {
    // Producer local: its only escape is the push (note 2).
    const row = new Row(i, i * 0.5);
    row.score = row.weight + 1;
    rows.push(row);
  }

  let total = 0;

  // Read form A: explicit indexed binding under an `i < rows.length` loop.
  for (let i = 0; i < rows.length; i++) {
    const s = rows[i];
    s.score = s.score + s.weight;
    total = total + s.score + s.id;
  }

  // Read form B: `for…of`, which desugars to the same bounded `rows[__idx]`.
  for (const r of rows) {
    total = total + r.rescore(2) + r.weight;
  }

  return total;
}

console.log("ptr_shape_elements:" + build(6));
