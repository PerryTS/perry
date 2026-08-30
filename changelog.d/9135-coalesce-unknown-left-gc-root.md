Fixed a moving-GC use-after-move for locals initialised with `x ?? null` (or
`?? <anything>`) where the left side is not statically typed — most commonly an
optional chain: `const masks = opts?.masks ?? null`. A Three.js world builder
compiled with Perry read from-space on its first copying minor (SIGBUS under
`PERRY_GC_PROTECT_FROMSPACE=1`, wrong vertex colours or an aborted build
otherwise).

Two defects, one rule. Both copies of the `??` type-inference rule
(`perry-hir` `analysis/value_types.rs` and `lower_types.rs`) answered the RIGHT
operand's type whenever the left was unknown, so the binding above was declared
`null`. The codegen pointer-locals collector ignores declared types by design
(#7846) and proves pointer-ness from the initializer — but it had no arm for
logical operators, fell back to that same inference, took `null` as proof, and
gave the local no GC root: the array's NaN-boxed address sat in a plain
`alloca double` across the loop poll and the next `masks[0]` dereferenced the
pre-collection address. An explicit `number[] | null` annotation could not help
(the collector distrusts annotations); a plain ternary was rooted all along.

The `??` rule is now one shared `coalesce_type`: an unknown left stays unknown,
a nullish left takes the right type, a nullable union gains the right's members.
The collector gets an explicit `Logical` arm that requires both operands to
classify before it answers, so a root decision never rides on the inference
again. Pinned by unit tests on the exact HIR, a lowering test, and a registered
`test_gap_gc_coalesce_local_root` witness (`?? null` over an array and over a
closure, live across loop polls).
