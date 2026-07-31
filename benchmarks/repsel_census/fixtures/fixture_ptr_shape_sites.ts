// Liveness fixture for the `Ptr<Shape>` CONSUMPTION SITES (#7106 follow-up).
//
// `fixture_ptr_shape.ts` proves the census can observe a promotion. This file
// proves something narrower and, it turned out, more urgent: that each codegen
// lowering which CONSUMES a `Ptr<Shape>` proof is exercised by the corpus at
// all.
//
// When per-site coverage was first measured, four of the six recorders fired
// and two had never fired on any workload in the census:
//
//   - `class_field_get_number.shape_proven_load` (expr/property_get/helpers.rs)
//     -- the bare fixed-offset load for a field that is NUMERIC-proven, as
//     opposed to merely shape-proven. Needs a class whose every reachable
//     store to the field is proven number-producing.
//   - `ptr_shape_update` (expr/instance_misc1.rs) -- `o.f++` lowered to
//     load/fadd/store at a fixed offset, with no by-name runtime call.
//
// Both are reachable. Nothing exercised them, so a break in either would have
// been invisible: the census counts promoted VALUES, and every value in the
// corpus was already consumed at some other site.
//
// What this program has to get right, all at once:
//
//   1. `c` must satisfy every `collectors/ptr_shape.rs` rule (see the sibling
//      fixture's header for the list) -- so: bound by one `Let` from a `new`,
//      never reassigned, captured, passed, returned or aliased.
//   2. Both fields must be NUMERIC-proven, which is what selects the
//      `class_field_get_number` load over the plain shape-proven one. Every
//      store is an integer literal or a proven-number expression.
//   3. `c` must survive SCALAR REPLACEMENT (#7115) -- otherwise the object is
//      deleted, no access site is reached, and the promotion is counted but
//      consumed nowhere. This is the delicate part. `c.v = c.v + 1` is what
//      does it: a plain in-loop field store defeats scalar replacement, and
//      dropping it takes this fixture to zero consumed sites. `c.v++` ALONE
//      does not -- an earlier draft with only the update was scalar-replaced
//      and exercised nothing.
//   4. `mix()` must be too complex for `simple_scalar_method_summary`, so the
//      method call does not re-enable scalar replacement, and it supplies the
//      `ptr_shape_method` site.
//
// Do not "tidy" this file. In particular do not fold `c.v = c.v + 1` into the
// `c.w++` update, and do not simplify `mix()`.

class Ctr {
  v: number;
  w: number;
  constructor() {
    this.v = 0;
    this.w = 1;
  }
  mix(): number {
    return this.v * this.v + this.w * this.w;
  }
}

function counted(n: number): number {
  let total = 0;
  for (let i = 0; i < n; i++) {
    const c = new Ctr();
    // Plain field store: defeats scalar replacement (see note 3).
    c.v = c.v + 1;
    // Field update: the `ptr_shape_update` site.
    c.w++;
    // Numeric-proven reads: the `class_field_get_number` site.
    // Method call on a proven receiver: the `ptr_shape_method` site.
    total = total + c.v * c.w + c.mix();
  }
  return total;
}

console.log("ptr_shape_sites:" + counted(4));
