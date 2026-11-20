### Fixed

**Materializing `Class.prototype` no longer disarms class-dispatch speculation and every element-shape proof for the rest of the process.**

`class_decl_prototype_value()` — the lazy materializer that creates a declared
class's prototype object on first demand — called
`invalidate_class_prototype_fast_guards()`. That is not a hint; it trips a
process-global, **monotonic** latch that:

* makes `js_method_direct_shape_guard` and
  `js_typed_feedback_method_direct_call_guard` return "miss" for every receiver,
  for the rest of the run, so every `recv.m()` on a declared class falls into
  the `js_native_call_method` dispatch tower;
* calls `crate::array::invalidate_all_element_shapes()`, retiring every
  outstanding element-shape record (#7480), so `arr[i]` reads fall back to the
  generic `js_require_object_coercible` + `js_is_symbol` +
  `js_object_get_index_polymorphic` path;
* bumps `VTABLE_GEN`, retiring the `vtable_ic` / `obj_dispatch_ic` dispatch
  caches (#7769).

The latch exists for the one event that can change which member `recv.m()`
resolves to: a **write** to a prototype (`Class.prototype.m = fn`). Those are
the two call sites in `class_registry/prototype_methods.rs`, and they keep it.
Reaching the materializer changes none of it — the object is fresh and
unobserved, and the writes immediately below install `constructor` plus exactly
the methods the class already declares, which are the same answers the vtable
already gives.

What reaches the materializer, measured with a name-printing probe on the
materializer itself: `new` on a class that `extends` anything, which
materializes the instance's whole prototype ancestor chain (`class B extends A`
+ `new B()` = 2 materializations; a three-level chain = 3). NOT `instanceof`
and NOT `Object.getPrototypeOf` — both trip zero. So an ordinary
class-hierarchy program disarmed its own dispatch speculation the first time it
constructed a subclass.

Measured on `gc-handoff/apps/shapes.ts` with a counter on each precondition of
the guard: **384,000 of 384,000 probes failed on this latch and on nothing
else** (`descriptors_in_use`, the GC-header checks, and the object-type check
all rejected zero). `gc-handoff/bench/shapes_dispatch.ts` shows the same for a
program containing no `instanceof` at all.

### Added

`js_method_direct_shape_class` — the class-id half of
`js_method_direct_shape_guard`, factored out so a call site can test more than
one `(class id, keys token)` pair per probe. `js_method_direct_shape_guard` is
now defined in terms of it, so the single-pair semantics are unchanged by
construction.

Codegen uses it to widen the shape-guarded direct call at a method callsite
from ONE arm (the declared receiver class) to the declared class plus its
subclass closure, each paired with the body the method resolves to when walked
from that class. The declared-class guard is a bet that the receiver's dynamic
class equals its static class; for a base-typed collection — `nodes: Node2D[]`
holding `Rect` / `Circle` / `Square` / `Marker` / `Group` — that bet loses on
every element. Arms are capped at `MAX_SUBCLASS_DISPATCH_ARMS` (8) so a wide
hierarchy keeps today's single-arm form rather than growing a long compare
chain, and only the shape-only guard is widened (the typed-feedback guard
records a single-contract observation per site and keeps its one arm).

### Notes for the next reader

`gc-handoff/bench/shapes_{build,describe,dispatch,dispatch_static}.ts` are the
committed decomposition of `apps/shapes.ts`, each annotated with its measured
seconds. They record, among other things, that **class dispatch is not where
`shapes.ts` loses**: on the quiet mini the whole `.area()` term is 0.013 s of a
0.224 s program, and widening the dispatch guard alone moved it 0.2237 →
0.2241 s. The two cost centres that decomposition does find are `build()`
(0.1035 s, 46%) and `describe()`'s string concatenation (0.074 s, 33%, ~620 ns
per `"lit" + this.stringField`).


### Blast radius

The latch is monotonic in production (the only `store(false)` is `#[cfg(test)]`),
and nearly every class-hierarchy program trips it, so the obvious worry is that it
silently disarms the element-shape repsel work (#7770/#7771/#7766/#7702). Measured:
it does not. `invalidate_all_element_shapes()` bumps a GENERATION; each record
carries the generation it was installed under and `ensure_element_shape`
re-establishes it on the next query, so one bump costs at most one
re-establishment per array. Adding a single `instanceof` or `Object.getPrototypeOf`
before an otherwise identical hot loop moves nothing: 0.0222 s for a `churn_read`
-shaped element-read loop over object literals AND over class instances, 2.46 s
for a method-call-per-element loop. Only the dispatch-guard half is permanent, and
on its own it is worth 1.0% on `shapes.ts`; it reaches 16.6% only combined with the
multi-arm widening.
