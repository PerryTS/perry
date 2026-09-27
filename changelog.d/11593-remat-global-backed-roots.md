Native root lowering no longer relocates a root slot whose only heap value is a
copy of an immutable, collector-rewritten global. It re-reads the global at each
use instead (`crates/perry-codegen/src/function/precise_roots/remat.rs`). Two
global families qualify. The first is string-literal handles (`<mod>_.str.N.handle`),
which covers `var X = "…"` locals. The second is class-keys arrays
(`@perry_class_keys_*`), which covers the entry-hoisted `new` caches from #7876.
Both are registered with `js_gc_register_global_root`, rewritten on evacuation,
and written only by their module's init. The pass checks the remaining
conditions on the emitted IR: every store into the slot is a load of that one
global or a constant, the slot's address never escapes, and the function never
stores to the global. A slot that fails any check stays an ordinary
`ptr addrspace(1)` root. A qualifying slot stays a plain alloca and holds a
`0x7FFC` marker. Each read becomes `select(slot == marker, load @G, slot)`, and
that select folds to a bare `load @G` wherever the store dominates the read.

On the Claude Code bundle's eight costliest functions, measured with the real
compiler on the same base commit, RS4GC relocations fell from 4.58 M to 0.75 M
(−83.5%). Post-RS4GC instructions fell from 5.68 M to 1.56 M, and the
statepoint rewrite took 27 s instead of 160 s. `__25747` alone went from
3.31 M to 0.32 M relocations, 3.66 M to 0.57 M instructions, 130 s to 13 s and
2.4 GB to 0.37 GB. Runtime instructions were unchanged or fewer on 11
benchmarks. They were −0.19% where the transform fires most.

Tests: `remat::tests` (plan, controls, RS4GC pipeline), and
`crates/perry/tests/remat_global_backed_roots.rs` +
`test-files/test_gap_remat_global_backed_roots.ts` (node-exact under default,
`PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1` with asserted
copying-minor relocations, a nursery sweep and a `PERRY_GEN_GC=0` control).
Sabotage: making the rewrite keep the stale stored address instead of
re-reading the global SIGSEGVs the default run.

`temp_root_operand_temporaries.rs`'s WTF-8 operand test is now split by
lowering. The shadow arm keeps the one-temp-root contract. Under native roots
the operand's slot is rematerialized, so the native arm checks #7114's
re-derivation on the value the concat receives: the marker is stored before the
allocating sibling, and every handle load the operand depends on sits below it.
The arm also rewires the concat to the pre-collection register and requires
that sabotaged IR to be rejected.
