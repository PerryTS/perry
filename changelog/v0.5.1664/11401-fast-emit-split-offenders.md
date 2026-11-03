perf(size): the fast-emit budget demotes only the functions over it, not their whole codegen unit (#10586).

A function over the optimized machine-pipeline budget
(`PERRY_LL_FAST_EMIT_MAX_INSTRS`; 100,000 on aarch64 and every unmeasured
target, 600,000 on x86-64) used to send its entire codegen unit through LLVM's
O0 machine pipeline, because a `TargetMachine`'s optimization level is
per-module and `optnone` does not reach LiveIntervals or the register
allocator. On a large module that is hundreds of ordinary siblings demoted
for one extreme function. That collateral — not the offender's own code — is
what made the ceiling's value so expensive to get wrong, and why arm64's
measured-dangerous 100k could only be argued against in terms of binary size.

`inprocess/split_emit.rs` now splits the unit after IR optimization: every
local symbol the offenders share with their siblings (the offenders
themselves, and any internal/private function or global an offender body
references) is renamed with a unit-unique suffix and promoted to a hidden
external; the module is cloned; the original keeps everything but the
offenders' bodies and is emitted through the optimized target machine, the
clone keeps only the offenders' bodies and is emitted through the O0 one, and
the two objects are combined exactly as codegen units already are
(`linker::finish_native_pieces` → `merge_unit_objects`), each finished through
the compact-map rewrite first under the statepoint plans. Neither half is
re-optimized, so the offenders' IR and machine code are what the old fallback
emitted and the siblings' code is what an undemoted unit emits. A unit whose
every function is over the budget, or (defensively; Perry emits none) that
has a global alias or ifunc, is still demoted whole, and the diagnostic says
which case applies. The ceiling values are unchanged.

Measured on the issue's own input, `typescript@5.9.3`'s `lib/_tsc.js`, Linux
x86-64 with `PERRY_LL_FAST_EMIT_MAX_INSTRS=100000` to reproduce the aarch64
default: three functions are over the budget (395,037, 171,883 and 124,273
instructions), and each used to demote its ~500-function unit (1,514
functions in total).
Only those three are demoted now; the linked binary goes
92,514,584 → 87,606,608 B (−4.9 MB, −5.3 %), with `tsc --version` and
`tsc --noEmit` over a type-error fixture byte-identical and exit code 2 in
both. The three affected units' emit time rises 15.7 → 35.6 s in total (their
~1,500 siblings now take the optimized pipeline, like every other unit's
functions); whole-module codegen was 12.7 vs 12.1 min and peak RSS 5.36 vs
5.43 GB, i.e. inside run-to-run noise on this host.

Tests: `the_budget_no_longer_makes_ordinary_siblings_pay` (replaces
`the_budget_is_what_makes_ordinary_siblings_pay`) asserts the sibling's
assembly under a crossed budget is byte-identical to the undemoted unit's,
with a control that the offender's code still differs from the optimized
pipeline's. `split_halves_share_promoted_locals_and_link_across_units` emits
two units whose offenders reach their siblings only through an internal
helper, an internal mutable global and a private string, partial-links each,
links both with a C `main`, runs the binary and checks its output against a
Rust model — sabotaged with a constant suffix it fails with duplicate
`helper.perry_fe.*` / `.str.perry_fe.*` definitions, and before the offenders
were re-identified by their promoted names it failed with undefined
references.
