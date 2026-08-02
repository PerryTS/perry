**GC root-dominance checker: `--unrooted-allocas` now distinguishes movability from rewritability (#7210).**

`_is_heap_source` answered *"does the collector rewrite this location?"* when the
reportable question is *"can the object this register names go bad while it sits
in unrooted memory across a collection?"*. #7210 measured the consequence: every
one of the 66 `--unrooted-allocas --moving-only` hits was a false positive, so the
population could never reach 0 and #7198's promote-to-required clock could not
start.

The predicate now models the two independent hazards — the object MOVES, or the
object is RECLAIMED — and exempts a source only when both are provably false. Two
exemptions qualify:

* **`@perry_class_keys_*` / `js_build_class_keys_array`** — allocated through
  `js_array_alloc_with_length_longlived` (old arena), which only old-page defrag
  relocates, and defrag short-circuits off (#6206); and the global is registered
  with `js_gc_register_global_root`, so the array is never unreachable.
* **`js_box_alloc*`** — `std::alloc::alloc`, outside the GC heap; `scan_box_roots_mut`
  rewrites the JSValue *inside* the box, never the box's address; and boxes are
  never freed.

Because an exemption is a suppression, it is gated rather than commented. A new
`--audit-immovable-sources` re-checks every premise against the runtime source and
goes red if one lapses; `--assume-old-defrag` / `--assume-boxes-in-gc-heap` restore
the reports so #7210's two counterfactuals are one flag away; `--self-test` proves
each exemption fires, is exactly reversed by its knob, and — the load-bearing arm —
still reports a **structurally identical fixture whose only difference is a nursery
allocator**. `--unrooted-allocas` also prints what each exemption suppressed, so a
clean corpus and a fully-exempted corpus no longer print the same line.

Measured over a fresh 134-file corpus at `c9cd73ba5`: **98 → 2**, with 96
suppressed (93 class-keys, 3 box). The 98 exceeds #7210's 66 because both the
corpus and `ALLOC_RE` grew since — #7227 added the `*_new*` convention.

The 2 residuals are **real** and are reported rather than allowlisted: a
`Symbol`-typed local gets no shadow slot because
`collectors/pointer_locals.rs`'s `is_definitely_non_pointer_type` lists
`Type::Symbol` as a non-pointer, while `alloc_symbol` is a `gc_malloc` of a
movable GC object. The gate becomes promotable to a required context once that
classification is fixed.
