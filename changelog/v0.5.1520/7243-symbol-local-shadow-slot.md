Fixed `collectors/pointer_locals.rs` classifying **`Type::Symbol` as a
non-pointer**, so a `Symbol`-typed local got no shadow-stack slot and lived in a
plain `alloca` — invisible to the precise root walk (#7236). `alloc_symbol` is
`gc_malloc(size_of::<SymbolHeader>(), GC_TYPE_STRING)` and `js_symbol_new`
returns it `POINTER_TAG`-boxed; nothing else holds a fresh symbol, so the malloc
sweep inside the copying minor frees one that is still live.

**The drift had already happened, one variant wide, inside one crate.**
`typed_shape::type_is_pointer_bearing` — an exhaustive match, and the function
that lays out the GC's own inline-field pointer masks — already answered `true`
for `Symbol` while `is_definitely_non_pointer_type` answered "non-pointer", so a
`Symbol` local was simultaneously not-a-pointer (no slot) and pointer-bearing
(scanned as one). That is verbatim what the doc comment over
`is_definitely_non_pointer_type` predicted would cost a use-after-move. There
was a third copy in `expr/shadow_slot.rs` carrying the same entry — a separate
hazard, since a `true` there suppresses `temp_root`'s protection of a symbol
operand held across an allocating call — and a fourth in
`lower_call/closure_analysis.rs` that correctly did not. All three now route to
one exhaustive definition, so a new `Type` variant is a compile error rather
than a silent "not a pointer". The full 21-variant audit found `Symbol` to be
the only misclassification; the other six non-pointers are NaN-boxed immediates
with no allocator.

**The failure is a premature FREE, not a stale address**, and the correction
matters because it dictates the witness. `gc_malloc` is the SYSTEM allocator
with a `GcHeader` in front, not an arena allocation, so the copying minor cannot
relocate a symbol — under #7235's taxonomy a `Symbol` local is RECLAIMABLE and
not MOVABLE (#7230's class, not #7019's). `SYMBOL_POINTERS` does not save it
either: `scan_symbol_pointer_metadata_roots_mut` uses
`visit_metadata_usize_slot`, which rewrites without marking, exactly as
`alloc_symbol`'s own comment says ("kept alive … or **not at all**"). Two
consequences are baked into the new witness: the pressure must be **symbols**,
because `copied_minor_malloc_sweep_due` gates the malloc sweep on a
`MallocCount` trigger while object churn reaches the *arena* trigger (object
churn reproduced 1 failure in 80 probes, and 0 on four of five repeats of
another shape); and identity probes report nothing, because `js_symbol_equals`
falls back to comparing `id` off the freed header and `js_is_symbol` to reading
`magic`.

New witness `test-files/test_gap_gc_symbol_local_rooting.ts`, registered in
`test-parity/gc_repsel_corpus.txt`: `A 30 B 20` at `f8f1e7188` on the
`loop_polls` arm (3/3 identical) **and on the shipped default** (2/2), `A 0 B 0`
after, byte-exact against node 26.5.1.

`gc_root_dominance_check.py --unrooted-allocas --moving-only` over a freshly
generated 118-source / 136-module corpus: **4 → 0** (the two #7236 hits in
`test_gap_class_forward_capture_6523` plus the new witness's two). That was 98
before #7235 and 2 after, and 0 is the condition #7198 named for promoting
`gc-root-dominance` to a required context. **The mode is now a step in that
job** rather than a number living in issue bodies — verified able to fail (exit
1 at `f8f1e7188`, exit 0 after) and sharing the corpus, exemptions, allowlist
and liveness floors with the existing dominance step. The promotion itself
remains a repo-admin action; `docs/src/internals/gc-rooting-invariant.md` now
carries the exact steps.

The representation-selection promotion census
(`compiler_output_regression.py census --gate`) is **byte-identical** across all
18 workloads before and after, so no promotion floor moved and no ratchet was
taken.
