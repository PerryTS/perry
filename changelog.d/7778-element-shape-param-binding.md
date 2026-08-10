**repsel: the element-shape loop clone serves the element-binding form through function boundaries (#7766).**

A fully-typed field read through a **parameter** array was still a by-name
lookup in two spellings the versioned clone declined: the explicit binding
(`const r = ps[i]; s += r.x + r.y`) and `for…of`, whose desugar emits exactly
that shape. The direct `ps[i].x` spelling was already served (the preheader
guard establishes the element shape at run time — the declared type only names
the class id to check against — so the mechanism was always sound for
parameters; it just could not see the binding form).

Three changes:

1. **Matcher** (`stmt/element_shape_loop.rs`): admits an optional leading
   `const r = arr[counter]` element binding whose every use is a tracked
   `r.field` read. Any other use — bare reference, mutable binding,
   non-counter index, a second array — declines, each with a red IR-census
   test. The fast clone **skips the binding's `Let` entirely** (lowering it
   would emit the element-read tier's calls, fail the call-free scan, and
   silently delete the clone — #7690's shape); `r.field` reads are answered
   by the loop fact (`ElementShapeLoopFact::element_binding_local`). The slow
   clone keeps the full body, so the side-exit protocol is unchanged.
2. **for-of desugar** (both emission sites): the minted counter init is
   `Integer(0)` instead of `Number(0.0)` — the literal kind
   `collect_integer_let_ids` seeds on, and the shape a user-written
   `let i = 0` produces. With `Number(0.0)` the desugared counter never
   joined `integer_locals`, never got a canonical i32 slot, and every
   i32-counter loop optimization silently declined the `for…of` spelling of
   loops it served in indexed form.
3. **`--opt-report`**: the clone records a `Ptr<Shape>` selection and
   per-read consumption when — and only when — the deref block branches into
   the fast clone, so a served parameter-array loop no longer reads as an
   unserved rule-1 wall.

Probes (200k elements × 200 passes): binding form through a parameter
0.22s → 0.07s, `for…of` 0.78s → 0.07s — both at node parity. Soundness is
tested, not argued: `test_gap_repsel_element_shape_param_binding.ts` runs
mixed arrays, a subclass, same-shape literals, a hole, mid-loop mutation and
a fake array-like through the boundary, byte-identical to node 26.5.1.
