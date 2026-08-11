### Performance

- **The element-shape versioned loop clone now fires on the idiom people actually write** (#7480 step 4, repsel/#5093).

  #7612 landed the clone and #7669 taught it object-literal element types. Neither
  moved `churn_read.ts` by a millisecond — it sat at 0.35 s across four measurement
  rounds while everything around it improved 4–11×. The lowering was never the
  problem. The **matcher** declined before reaching it, for two independent reasons,
  each on its own fatal:

  1. **`for (let j = 0; j < arr.length; j++)` failed the bound match.** The bound
     admitted a literal or a loop-invariant local; `keep.length` is a `PropertyGet`,
     so the match returned `None` at the condition, before the class resolver was
     ever consulted. That is the bound form every one of #7480's own kernels — and
     `churn_read`, `churn`, `retain` — is written in.
  2. **`type Node = { v: number; w: number }` shadowed the anon-shape resolver.**
     `element_class_name` returns `receiver_class_name`'s answer when it has one, and
     for an alias-typed array that answer is `"Node"` — a name no `ctx.classes` entry
     owns, because the literals allocate an `__AnonShape_…`. The early return meant
     the resolver #7669 added for exactly this case was never reached for any
     alias-typed array.

  Both are fixed here. `element_class_name` only takes the named answer when it names
  a real class; the anon-shape resolver follows `type` aliases (bounded, on both the
  array and the element level, so `type Row = Node[]` resolves too). A new
  `ElementShapeLoopBound::ArrayLength` admits `j < arr.length` **for the array the
  body reads** — a foreign array's length is declined, because the preheader proves
  nothing about how two lengths relate and the result would be an out-of-range read
  rather than a slow clone.

  The `.length` trip count costs nothing to materialize: the guard's deref block
  already loads `ArrayHeader.length`, so the usual `length >= bound` comparison
  collapses and the only remaining obligation is that the `u32` fits a non-negative
  `i32` (one `icmp`; the clone's counter is an i32 and its trip test is signed).
  Hoisting `arr.length` out of the condition is not something JS licenses in general
  — the property is re-read every iteration — and it is sound here for the same
  reason the whole clone is: the matcher admits no store and no call in the body, and
  every way to change an array's length is one or the other. The slow clone keeps
  re-reading it.

- **A guarded fast clone no longer emits the two calls that silently delete it.**

  `lower_element_shape_versioned_for` and `lower_class_field_versioned_for` both
  build their fast clone first and prove it call-free second; when the proof
  fails they terminate the guard with an *unconditional* branch to the slow
  clone and leave the fast blocks as unreachable code. So a call emitted into
  one of these clones does not make it slower — it removes it, silently, with
  every block label still present for an IR census to find. Two lowerings were
  doing exactly that to the element-shape clone:

  - **The GC back-edge poll.** #7690 restored moving-loop polls to ON by
    default, putting a `js_gc_loop_safepoint()` in the clone's element-load
    block. Measured on `churn_read.ts`: with polls off the clone runs (0.03 s
    user); with polls on the guard emits `br label %element_shape.loop.slow.preheader`
    and the program runs the generic body (0.54 s). Skipping the poll inside a
    clone is not a new licence — a poll exists so an *allocating* loop can defer
    a collection, which is why `loop_may_allocate` already gates it. That
    predicate answers from the HIR body, before specialization, so it cannot see
    that `arr[j].f` lowers to a bare load here. Inside a clone's fact scope it
    can: the clone is call-free or it is not entered, and the slow clone —
    lowered after the scope is popped — keeps its poll.
  - **The loop-invariant `arr.length` hoist.** With a `.length` bound the clone
    got a *second* `plen` diamond (`js_value_length_f64`) whose result nothing
    reads, because the caller had already handed the trip count in as
    `precomputed_i32_bound`. Only the load is skipped; the bounded-index and
    buffer-width facts and the i32 counter slot are proofs and storage the
    clone's other lowering may depend on.

  This is the failure mode `stmt/element_shape_loop.rs`'s module docs named in
  advance — "a silent loss of the optimization … with no test failing" — and
  every `perry-codegen` test was green throughout. `assert_fast_clone_is_entered`
  now asserts the guard branches *into* the clone, and it is sabotage-tested:
  with the suppression removed, the two `.length`-bound tests go red with
  "the deref block ends in an unconditional branch to the slow clone", so a green
  run is evidence rather than decoration.

  **The class-field clone is not affected today**, and that was measured rather
  than assumed: with the suppression removed its three IR tests stay green,
  because `loop_may_allocate` already proves an `obj.field`-only body inert and
  emits no poll for it. The suppression covers it anyway — the two clones rest on
  the identical argument — and its tests gained the same liveness assertion, plus
  the same by-name slice fix, so the next body shape admitted to that matcher
  cannot delete its clone unnoticed either. A third instance,
  `lower_object_array_write_versioned_for`, has the same scan and no fact scope
  to key off; it is a write-path loop, is not covered by the benchmarks measured
  here, and is left for a follow-up rather than changed blind.

### Fixed

- **`fast_clone_slice` could attribute a neighbour's call to the fast clone**
  (`stmt/element_shape_loop_tests.rs`).

  The IR census sliced "everything between the fast clone's cond block and the slow
  clone's", which is the fast clone only while nothing else is emitted in between. An
  `arr.length` bound makes the slow clone hoist its own length read, and those
  `plen.*` blocks land in the gap — so the call-free census failed on a fast clone
  that was, and still is, bare. It now selects blocks by name (`for.element_shape_fast.*`
  and `element_shape.load`), keeping the anti-vacuity assertion that made it a gate.
  This is the mirror image of the #7480 step 3 bug the function's own doc comment
  records: a span that depends on what a neighbour emits can report wrong in either
  direction, so ownership is asked of the block.

  New coverage: the `.length` bound fires and stays call-free; a foreign array's
  `.length` is declined; an aliased element type resolves; an unresolvable `Named`
  element type with no class and no alias is declined rather than guessed; and
  `churn_read.ts`'s exact shape (alias **and** `.length` together) reaches the clone.
