A hot function that folds a typed-array **parameter** into an accumulator —
`function reduce(arr: Float64Array) { let s = 0.0; for (…) { let x = arr[i] + 1.0;
s = s + x; } }` — no longer keeps its numeric locals as NaN-boxed GC roots.

The spec-ABI already proves such a parameter (`TaPtr`) permanently holds one
specific numeric-kind, non-view typed array, and specializes the body so the
element read inlines. But the `number_by_construction` fixpoint
(`collectors/ptr_shape_numeric.rs`) only recognised a typed-array element read as
"a Number or `undefined`, never a pointer" for a **local** view with a
compiler-visible `TypedArrayNew` initializer — not for a proven `TaPtr`
parameter. So the fresh, read-derived `x` failed the numeric proof, which
cascaded to the loop-carried accumulator `s = s + x`. Both then kept a shadow
root slot with a per-write `js_write_barrier_root_nanbox`, and `s`'s update
lowered to the opaque `js_dynamic_string_or_number_add` call instead of an inline
`fadd`.

The fixpoint now also treats a read off a `spec_ta_lens` binding as
Number-or-`undefined`. `spec_ta_lens` is keyed exactly by `SpecParamRep::TaPtr`
parameters, and `collectors::spec_abi_sites` admits a `TaPtr` only for
`spec_ta_kind_is_numeric` kinds (the BigInt typed arrays — whose elements are
BigInt pointers — are never `TaPtr`), so `arr[numeric_index]` off one is provably
a Number in-bounds and `undefined` out of range, which `+`/`-` launders into a
genuine Number (`NaN` at worst). The `rec(index)` guard is retained — a
non-numeric key would read a property, which can be a pointer. Soundness rests on
the entry contract, not on the erased `Float64Array` annotation, so a reassigned
or unproven receiver is untouched.

Effect on a 200000×4096 `Float64Array` reduction passed by parameter: the
accumulator's per-iteration `js_dynamic_string_or_number_add` and root barrier
become a single `fadd` in a raw `double` slot — ~5× faster (measured 5.1–7.3s →
~1.0s), with byte-identical output to the rooted build under every moving-GC
configuration and to Node.

Does not yet cover a typed array read through a **module-global** binding (the
issue #8619 reproducer): on `main` that read is still a runtime call (module-
global read inlining, #8617, is unmerged), so its accumulator rooting is a
secondary cost there; extending the same proof to `module_global_proven_types`
is the natural follow-up once the read inlines.
