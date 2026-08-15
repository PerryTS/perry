### Fixed

- Preserve production Next.js App Route request state through generated
  `AppRouteRouteModule.handle` dispatch, imported handlers, async continuations,
  separate runtime providers, and verified moving garbage collection.

- Cap the in-process optimization cost of statepoint relocation fan-out: a
  single 51k-line minified Next chunk closure grew 40x to 2.1M instructions
  under `rewrite-statepoints-for-gc`, and one `-Os` function pass then ran for
  over an hour on it. Post-rewrite, functions past 512k instructions (tunable
  via `PERRY_LL_RS4GC_OPTNONE_INSTRS`) are stamped `optnone`+`noinline` so the
  pipeline skips exactly them; the affected unit now finishes in ~21s. The
  external text path already re-derived its #4880 opt tier from the rewritten
  text and needed no change.

- Exempt the empty inline-asm loop-preservation barrier from
  `rewrite-statepoints-for-gc` (`"gc-leaf-function"` at all three emission
  sites). RS4GC statepoint-wrapped the barrier into verifier-invalid IR
  ("Cannot take the address of an inline asm!"), which the in-process
  pipeline fed to ISel unverified — a bare SIGBUS with no diagnostic. The
  in-process rewrite now verifies its output so any future invalid shape
  fails loudly, and LLVM unit workers get 64 MiB stacks so deep pass/ISel
  recursion on relocation-grown functions cannot hit a 2 MiB guard page.

- Claim action-zero landing pads as Perry catches again. The review-driven
  Handler/Cleanup discrimination assumed catch handlers always carry a
  non-zero LSDA action, but #7982's statepoint retype makes every JS catch
  pad a `landingpad token cleanup` (action zero) under default native-roots
  builds — so phase one skipped every statepoint-built catch and a plain
  `try { throw } catch` aborted FATAL "no landing pad". The strict
  transactional LSDA parsing stays; `PERRY_EH_TRACE=1` now prints per-frame
  personality decisions for future hunts.

- Keep native statepoint roots in app dylibs: the demotion of `--output-type
  dylib` artifacts to the shared shadow stack predated #8081's loaded-image
  stack-map indexing and would leave provider apps running a lowering
  production never ships.
