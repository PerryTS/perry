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

- Reserve 64 MiB stacks for LLVM codegen-unit workers. Pass and ISel
  recursion scales with function size, and a relocation-grown function
  overflowed the default 2 MiB worker stack — a guard-page SIGBUS with no
  crash report. The reservation is address space, not resident memory.
