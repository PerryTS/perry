### Fixed

**GC rooting: a parameter's caller write, and two allocate-then-fill lowerings
that never got a rooting contract (#7280, #7154).**

Three violations of the invariant at the top of
`docs/src/internals/gc-rooting-invariant.md` — a GC-managed value live across a
collection point must be reachable from a root before that point — all found by
running `scripts/gc_root_dominance_check.py --stale-registers --moving-only`
over a **dependency-scale** corpus rather than the 25-file curated one. #7284's
correction to `POLL_CAPABLE_RUNTIME` is what made the property-GET half of that
report readable at all: on the gate corpus `--moving-only` went 65 → 115, and
101 of the 115 are a single unaddressed rule (a shadow-slot load cached in a
register across a collection point).

**A parameter's incoming argument is a write the analysis never saw.**
`collect_pointer_typed_locals`' refinement fixpoint proves a local non-pointer
from its `writes`, which is collected by walking the body — so for a *parameter*
it reasons from a strict subset of the local's definitions. The
optional-parameter desugaring then supplies, for free and on every optional
parameter in the program, the one write that completes the false proof:
`if (p === undefined) { p = undefined; }` is `Type::Void`, definitely
non-pointer, so `all_non_pointer` held and the parameter lost its shadow slot
while its declared type said `Object`. Measured on zod's
`clone(inst, def?, params?)`: `js_shadow_frame_enter(2)` with slot 0 ← `inst`
and no bind at all for `def` or `params`, so LLVM promoted `params` into
callee-saved `d8`. Fixed by seeding the caller's write
(`LocalWrite::Incoming(declared_ty)`) rather than special-casing parameters at
each conclusion, so every consumer of the fixpoint accounts for it without
having to know it must; a `number` parameter is still provably non-pointer,
because its declared type is the one thing that does constrain the caller. Same
defect as #7291's site (1), found independently and behaviourally equivalent to
it (identical frame size and bind set on the same probe).

**The spread-array accumulator** (`Expr::ArraySpread`) allocates up front and
lowers the elements *into* the array — the opposite order from
`lower_array_literal`, which lowers every element first into a temp root and
only then allocates. Threading the accumulator through each
`js_array_push_f64` return value handles *reallocation* and does nothing for
*relocation*: for `[a, ...gen(), b]` the half-built array was live across an
iterator protocol while reachable from no root at all, so a minor reclaimed it
rather than moving it, and the remaining appends wrote into recycled memory.
Rooted with `temp_root_set_i64`, because the accumulator's address legitimately
changes on every append (the string-concat contract from #6971).

**The namespace-import object** (`import * as ns` used as a value) materializes
one property per export of the source module. Every other allocate-then-fill
lowering has carried a rooting contract since #6951 (`Expr::Object`) or #7211
(class objects); this one, added for #629's Drizzle/Stripe namespace
enumeration, never got one — and it builds the largest object in a
dependency-scale program. Both halves of its loop are collection points:
resolving `ns.member` for a `const` export is a call into the exporting module's
accessor, and `js_object_set_field_by_name` performs the allocating keys-array
transition. Stock zod's `import * as core` materializes 269 members here, and
the emitted IR carried zero `js_gc_temp_root_*` calls beside its 269 allocating
stores.

`gc/policy.rs`'s "sound by construction" claim about the loop-polls route is
also corrected (#7280 ask 2): deferring to a precise safepoint makes the
*collector* precise and says nothing about whether the mutator's live values are
reachable from the root set that safepoint scans.

**These do not fix #7280, and #7280 stays open.** Its reproducer is unchanged at
**0/30** on the revert configuration (`PERRY_GC_MOVING_LOOP_POLLS=1` compiled
*and* run) and **0/10** on the allocation-point arm with movement asserted
(10/10 runs `copied_objects>0`); the stock-zod control reads 31/40 against a
34/40 baseline, which is inside the noise band that configuration has shown
(31, 32, 34, 35, 35 across five independent builds). The shipped default is
unaffected: 30/30 and 40/40. #7291 measures 0/30 and 32/40 on the same harness,
so it does not close the acceptance test either. After the parameter fix the
`PERRY_GC_PROTECT_FROMSPACE_DEPTH=800` quarantine fault moves off zod's `clone`
and lands in `js_native_call_method` called from a compiled module-init body — a
receiver going stale in a *runtime* frame, which is #7249's blind spot.

No witness ships with this change. One was written for the two accumulator
fixes and confirmed by IR inspection to exercise both lowerings, then discarded:
it is 20/20 clean on the parent under `loop_polls` and 10/10 clean on the
allocation-point arm with movement asserted, so it does not discriminate. A test
that passes on the parent is a dark test with a witness's name on it, which is
what #7278 exists to stop. The only artifact that discriminates for this class
remains the dependency-scale reproducer #7280 preserved.
