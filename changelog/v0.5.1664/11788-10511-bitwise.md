perf: bitwise ops on destructured or annotated locals run native inside a Number-versioned loop clone (#10511)

Two changes:

1. Unary `~` on an operand the compiler cannot prove is a Number now takes the
   same inline guard as the binary bitwise operators (`|v| < 2^63`, with the
   exact `js_dynamic_bitnot` on the cold arm), instead of calling the helper
   on every evaluation. BigInt, `valueOf` and throwing coercions keep their
   exact semantics on the cold arm.

2. New loop tier `stmt/number_local_loop.rs`. In a receiver-free loop whose
   bitwise operators read locals with no function-scope Number proof (noble's
   `let { A, B, C, D } = this` SHA-2 round), a local is Number at every read
   inside the loop when (1) it holds a Number at entry, which one inline test
   checks, (2) every write that can execute in the loop, in the body AND the
   condition AND the update clause, produces a Number given the admitted
   locals (a greatest fixed point), and (3) no closure, `await`, `yield`,
   boxed cell or module global can write it. The loop then runs in a clone
   whose 5L Number scope holds those locals, each in a plain F64 alloca that
   is written back on exit. When the entry test fails, the ordinary loop runs.

Result (instr/iter): `destructure` 212 to 57 and `annotated` 213 to 57 (node
41). `prop` (78) and `bigmask` (186) are dominated by `any`-receiver property
reads that are not hoisted, not by bitwise ops, and are unchanged. Tests: IR
tests in `stmt/number_local_loop_tests.rs` and
`expr/unary_bitnot_tests.rs`. Sabotaging the condition/update walk or the fixed
point turns the clause and concatenation witnesses red. The gap test
`test_gap_10511_number_local_loop.ts` covers ToInt32 edge values, non-Number
entries, clause writes, break/return/throw exits, nested loops, and BigInt
TypeErrors.
