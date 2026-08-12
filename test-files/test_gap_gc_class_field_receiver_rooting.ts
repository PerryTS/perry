// #7640 section C: `obj.field = <allocating RHS>` on a known class evaluates
// the receiver before the value (spec order) — every arm in
// `expr/property_set.rs`'s class-field family (`try_lower_sloppy_class_field_store`,
// the strict `class_field_global_index` fast path, and the setter-dispatch
// arm) lowers the receiver first and reuses that register after the RHS runs.
//
// Two of those arms carried a comment claiming this is already safe: "the
// receiver's relocation across an allocating RHS is handled by the same
// statepoint re-read that arm relies on." The answer turned out to split in
// two, both confirmed against this file's IR:
//
//  * `object` a bare local/`this` (`setRawF64`, `setBoxed`, `setViaSetter`
//    below) — TRUE, but not for the stated reason. There is no
//    statepoint-specific re-read; what actually closes the window is
//    `root_reload.rs` (#7280), a front-end pass that re-materialises a value
//    derived from a shadow-slot or handle-global load below any collection
//    point it cannot dominate. It runs before either root lowering sees the
//    IR, so it protects the shadow (`PERRY_RS4GC=0`) and native
//    (`PERRY_RS4GC=1`, default) backends identically.
//  * `object` a compound receiver — a class-field READ, `this.target.x = …`
//    (`Runner.run` below) — FALSE. The receiver register is the `phi` result
//    of a class-field GET, not a direct shadow-slot load, so `root_reload`
//    has nothing to re-derive from, and it is reused unreloaded after the
//    RHS's allocating call in the emitted IR (confirmed by hand). This shape
//    stays genuinely open — see the note in `property_set.rs` and
//    changelog.d/ for why it is not fixed in the same change (the fix is a
//    measured-cost one on a hot path, the finding is not).
//
// This test is the permanent corpus member for both halves: it exercises the
// receiver-then-allocating-value shape on plain field (raw-f64 and boxed),
// an accessor (`set` method) dispatch, and the compound `this.target.field =`
// shape, under enough allocation pressure (`PERRY_GC_MOVING_LOOP_POLLS=1`
// back-edge polls) that a moving minor is reachable inside the RHS, and
// asserts the stored values are never stale. The safe arms pin the finding;
// the `Runner.run` arm keeps the open shape in the corpus for when an
// instrument that can see it exists.
//
// Verified directly against `scripts/gc_root_dominance_check.py` on this
// exact shape (both `--stale-registers` and `--statepoints`, both lowerings):
// zero hazards. See changelog.d/ for the full note.

class Point {
  x: number = 0;
  next: Point | null = null;
  _y: number = 0;
  set y(v: number) {
    this._y = v;
  }
}

class Holder {
  target: Point = new Point();
}

// Allocation pressure: enough garbage that a loop back-edge poll inside this
// function can land a moving minor while the caller's receiver register is
// still holding a pre-collection address.
function churn(seed: number): number {
  const bits: unknown[] = [];
  for (let i = 0; i < 600; i++) {
    bits.push({ i: i, s: "x" + i });
  }
  return seed + bits.length - 600;
}

function allocPoint(n: number): Point {
  const p = new Point();
  p.x = n;
  return p;
}

// Sloppy-mode raw-f64 class field: `try_lower_sloppy_class_field_store`'s
// requires_raw_f64 arm.
function setRawF64(p: Point, n: number): void {
  p.x = allocPoint(churn(n)).x;
}

// Sloppy-mode boxed class field: `try_lower_sloppy_class_field_boxed_store`.
function setBoxed(p: Point, n: number): void {
  p.next = allocPoint(churn(n));
}

// Setter dispatch (`property_set.rs`'s `ctx.methods.get(&setter_key)` arm) —
// no rooting comment at all before this test.
function setViaSetter(p: Point, n: number): void {
  p.y = allocPoint(churn(n)).x;
}

// `this.target.field = <allocating>`: the receiver of the innermost
// PropertySet is `this.target`, itself a PropertyGet result rather than a
// bare local — still reached by the same class-field arms.
class Runner {
  run(p: Point, n: number): void {
    const h = new Holder();
    h.target = p;
    h.target.x = allocPoint(churn(n)).x;
    h.target.next = allocPoint(churn(n + 1));
  }
}

function main(): void {
  let bad = 0;
  const runner = new Runner();
  for (let r = 0; r < 300; r++) {
    const p = new Point();
    setRawF64(p, r);
    if (p.x !== r) bad++;

    setBoxed(p, r);
    if (p.next === null || p.next.x !== r) bad++;

    setViaSetter(p, r);
    if (p._y !== r) bad++;

    runner.run(p, r);
    if (p.x !== r) bad++;
    if (p.next === null || p.next.x !== r + 1) bad++;
  }
  console.log(bad);
}

main();
