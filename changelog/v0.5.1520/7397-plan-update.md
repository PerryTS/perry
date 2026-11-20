**Updated** `docs/engine-plan.md` with the 2026-08-04 findings.

Two things the plan treated as measured were not. Statepoints could not compile
on aarch64-ELF at all — a hard failure on a default-on path, from two stacked
bugs (#7390) behind two toolchain ones (#7384, #7388). And three of the four
RS4GC matrix arms had **never executed**, in any run, for want of a `concurrency`
group (#7393) — so every "the ELF arm is the only one red" conclusion rested on
arms that never ran.

Also folded in: nine Layer 3 rooting fixes and the rule they share (ordering, not
missing roots; a fault that moves is a real fix, one that does not move was
already dead); #7380's type confusion and the `gc_type == GC_TYPE_OBJECT`
generalisation; RSS −69% (#7377); and the first honest performance measurement,
including two benchmarks that measure nothing (#7395) and the array-store guard's
siting cost (#7396).

The Layer 1 framing is corrected: `lower_exprs_rooted` already implements the
RFC's proposal for codegen operands, so the gap is Layer 3, where #7389 supplies
the first structural answer.
