### Fixed

- Exempt the empty inline-asm loop-preservation barrier from
  `rewrite-statepoints-for-gc`. RS4GC wrapped it into a `gc.statepoint` whose
  callee is inline asm — IR the verifier rejects ("Cannot take the address of
  an inline asm!"). The external `opt` path aborts on that; the in-process
  path ran no post-rewrite verify and fed the broken module to ISel, where it
  died as a bare SIGBUS with no diagnostic. The barrier now carries
  `"gc-leaf-function"` at all three emission sites (an empty asm can never
  reach a safepoint), and the in-process pipeline verifies after the rewrite
  so a future invalid shape fails loudly instead of crashing the backend.

- Cap the in-process optimization cost of statepoint relocation fan-out. The
  #4880 opt-tier decision is made from pre-rewrite sizes, but one 51k-line
  minified-bundle closure grew 40x to 2.1M instructions under RS4GC and a
  single `-Os` function pass then ran over an hour on it. Post-rewrite,
  functions past 512k instructions (tunable via
  `PERRY_LL_RS4GC_OPTNONE_INSTRS`, registered as a build-cache key) are
  stamped `optnone`+`noinline`, so the pipeline skips exactly the exploded
  functions and still optimizes their siblings; the affected unit now
  finishes in ~21s. `optnone` gates only the middle-end, leaving the
  statepoint lowering and compact GC map unchanged.

- Reserve 64 MiB stacks for LLVM codegen-unit workers. Pass and ISel
  recursion scales with function size, and a relocation-grown function
  overflowed the default 2 MiB worker stack — a guard-page SIGBUS with no
  crash report. The reservation is address space, not resident memory.
